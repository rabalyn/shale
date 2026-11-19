//! Date and time helpers for the calendar, without a date library:
//! civil dates ↔ day numbers, local time through the C library, and named time zones (TZID)
//! read from TZif data: the system's zoneinfo files, on Windows the database bundled in the program.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::tzrule::Rule;

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The day number of `2026-11-12` (a valid civil date), else none.
pub fn parse_ymd(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let y: i32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next()?.trim().parse().ok()?;
    let d: u32 = it.next()?.trim().parse().ok()?;
    ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m)).then(|| days_from_civil(y, m, d))
}

/// Civil date for a day number.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// 0 = Monday … 6 = Sunday.
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    (next - days_from_civil(y, m, 1)) as u32
}

pub fn add_months(y: i32, m: u32, n: i32) -> (i32, u32) {
    let t = y * 12 + (m as i32 - 1) + n;
    (t.div_euclid(12), (t.rem_euclid(12) + 1) as u32)
}

pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Seconds east of UTC of the computer's local time zone at the UTC instant `utc`.
pub fn local_offset(utc: i64) -> i64 {
    #[cfg(unix)]
    {
        let t: libc::time_t = utc as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
            return tm.tm_gmtoff as i64;
        }
    }
    #[cfg(windows)]
    if let Some(off) = windows_offset(utc) {
        return off;
    }
    let _ = utc;
    0
}

/// The same on Windows, from the system's time zone settings (summer time included).
#[cfg(windows)]
fn windows_offset(utc: i64) -> Option<i64> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
    let (days, sod) = (utc.div_euclid(86_400), utc.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    // the range the Windows calls take
    if !(1601..=30_827).contains(&y) {
        return None;
    }
    let st = SYSTEMTIME { wYear: y as u16, wMonth: m as u16, wDayOfWeek: 0, wDay: d as u16, wHour: (sod / 3600) as u16, wMinute: (sod % 3600 / 60) as u16, wSecond: (sod % 60) as u16, wMilliseconds: 0 };
    // SAFETY: both structs are valid for the call; a null zone means the computer's own.
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &st, &mut local) } == 0 {
        return None;
    }
    let wall = days_from_civil(local.wYear as i32, local.wMonth as u32, local.wDay as u32) * 86_400 + local.wHour as i64 * 3600 + local.wMinute as i64 * 60 + local.wSecond as i64;
    Some(wall - utc)
}

/// Local wall clock → UTC (handles summer time changes).
pub fn local_to_utc(days: i64, secs_of_day: i64) -> i64 {
    wall_to_utc(days * 86_400 + secs_of_day, local_offset)
}

/// UTC → (local day number, seconds since local midnight).
pub fn utc_to_local(utc: i64) -> (i64, i64) {
    let w = utc + local_offset(utc);
    (w.div_euclid(86_400), w.rem_euclid(86_400))
}

/// Local day number of "now".
pub fn today() -> i64 {
    utc_to_local(now()).0
}

/// The one clock of the app (UTC seconds): the real time, except in screenshot mode with
/// `SHALE_FAKE_NOW` set (see `freeze`), so that pictures do not change from day to day.
pub fn now() -> i64 {
    FROZEN.get().copied().unwrap_or_else(real_now)
}

fn real_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

static FROZEN: OnceLock<i64> = OnceLock::new();

/// Screenshot mode only: from now on `now()` answers `secs`. Only the first call counts.
pub fn freeze(secs: i64) {
    let _ = FROZEN.set(secs);
}

/// The frozen time, if screenshot mode froze it.
pub fn frozen() -> Option<i64> {
    FROZEN.get().copied()
}

/// "2026-11-15T10:00:00" (UTC; the seconds and the T may be left out or be a space) as UTC seconds.
pub fn parse_fake_now(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once(['T', ' ']).unwrap_or((s, "00:00:00"));
    let mut d = date.split('-').map(|n| n.parse::<u32>().ok());
    let (y, m, dd) = (d.next()??, d.next()??, d.next()??);
    if d.next().is_some() || !(1..=12).contains(&m) || dd < 1 || dd > days_in_month(y as i32, m) {
        return None;
    }
    let mut t = time.split(':').map(|n| n.parse::<i64>().ok());
    let (h, mi, se) = (t.next()??, t.next().unwrap_or(Some(0))?, t.next().unwrap_or(Some(0))?);
    if t.next().is_some() || !(0..24).contains(&h) || !(0..60).contains(&mi) || !(0..60).contains(&se) {
        return None;
    }
    Some(days_from_civil(y as i32, m, dd) * 86_400 + h * 3600 + mi * 60 + se)
}

fn wall_to_utc(wall: i64, offset_at: impl Fn(i64) -> i64) -> i64 {
    let mut utc = wall - offset_at(wall);
    for _ in 0..2 {
        utc = wall - offset_at(utc);
    }
    utc
}

/// A zone read from a TZif file: transition times and the offset in effect after each, and the
/// rule from the file's footer for the time after the last transition.
struct Zone {
    transitions: Vec<(i64, i64)>,
    initial: i64,
    footer: Option<Rule>,
}

impl Zone {
    fn offset(&self, utc: i64) -> i64 {
        if let (Some(rule), true) = (&self.footer, self.transitions.last().is_none_or(|last| utc >= last.0)) {
            return rule.offset(utc);
        }
        match self.transitions.binary_search_by(|t| t.0.cmp(&utc)) {
            Ok(i) => self.transitions[i].1,
            Err(0) => self.initial,
            Err(i) => self.transitions[i - 1].1,
        }
    }

    /// The changes of the offset in [from, to): (UTC instant, offset before, offset after).
    fn changes(&self, from: i64, to: i64) -> Vec<(i64, i64, i64)> {
        let mut out = Vec::new();
        let mut before = self.initial;
        for (at, after) in &self.transitions {
            if *at >= to {
                break;
            }
            if *at >= from && *after != before {
                out.push((*at, before, *after));
            }
            before = *after;
        }
        // past the last transition the changes come from the rule of the footer
        if let Some(rule) = &self.footer {
            let last = self.transitions.last().map_or(i64::MIN, |t| t.0);
            let year_of = |t: i64| civil_from_days(t.div_euclid(86_400)).0;
            let (first_year, last_year) = (year_of(from.max(last)) - 1, year_of(to) + 1);
            let mut ats: Vec<i64> = (first_year..=last_year).flat_map(|y| rule.changes_in_year(y)).map(|c| c.0).filter(|at| *at > last && (from..to).contains(at)).collect();
            ats.sort();
            out.extend(ats.into_iter().map(|at| (at, self.offset(at - 1), self.offset(at))).filter(|c| c.1 != c.2));
        }
        out
    }
}

fn parse_tzif(data: &[u8]) -> Option<Zone> {
    if data.len() < 44 || &data[0..4] != b"TZif" {
        return None;
    }
    let be32 = |o: usize| -> Option<u32> { data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])) };
    let counts = |base: usize| -> Option<[usize; 6]> { Some([be32(base + 20)? as usize, be32(base + 24)? as usize, be32(base + 28)? as usize, be32(base + 32)? as usize, be32(base + 36)? as usize, be32(base + 40)? as usize]) };
    let [isutc, isstd, leap, timecnt, typecnt, charcnt] = counts(0)?;
    let v1_len = 44 + timecnt * 4 + timecnt + typecnt * 6 + charcnt + leap * 8 + isstd + isutc;
    // prefer the 64-bit section of version 2+ files
    let (base, tsize) = if data[4] >= b'2' && data.len() > v1_len + 44 { (v1_len, 8) } else { (0, 4) };
    let [isutc, isstd, leap, timecnt, typecnt, charcnt] = counts(base)?;
    let mut o = base + 44;
    let mut times = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        let t = if tsize == 8 { i64::from_be_bytes(data.get(o..o + 8)?.try_into().ok()?) } else { be32(o)? as i32 as i64 };
        times.push(t);
        o += tsize;
    }
    let idx: Vec<usize> = data.get(o..o + timecnt)?.iter().map(|b| *b as usize).collect();
    o += timecnt;
    let mut types = Vec::with_capacity(typecnt);
    for _ in 0..typecnt {
        types.push(be32(o)? as i32 as i64);
        o += 6;
    }
    let transitions = times.into_iter().zip(idx).map(|(t, i)| (t, *types.get(i).unwrap_or(&0))).collect();
    // after the abbreviations, leap seconds and indicators of the 64-bit section: "\n<TZ rule>\n"
    let footer = (tsize == 8)
        .then(|| {
            let end = o + charcnt + leap * 12 + isstd + isutc;
            let tail = data.get(end..)?.strip_prefix(b"\n")?;
            Rule::parse(std::str::from_utf8(&tail[..tail.iter().position(|b| *b == b'\n')?]).ok()?)
        })
        .flatten();
    Some(Zone { transitions, initial: *types.first().unwrap_or(&0), footer })
}

fn zone(tzid: &str) -> Option<std::sync::Arc<Zone>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<std::sync::Arc<Zone>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(z) = cache.lock().unwrap().get(tzid) {
        return z.clone();
    }
    let clean = tzid.trim_matches('"').trim_start_matches('/');
    #[cfg(windows)]
    let z = bundled_zone(clean);
    #[cfg(not(windows))]
    let z = system_zone(clean);
    let z = z.map(std::sync::Arc::new);
    cache.lock().unwrap().insert(tzid.to_string(), z.clone());
    z
}

/// A zone from the zoneinfo files of the system (none on Windows). Names that are paths are refused.
#[cfg(not(windows))]
fn system_zone(name: &str) -> Option<Zone> {
    if name.contains("..") {
        return None;
    }
    ["/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"].iter().find_map(|dir| crate::fsutil::read(std::path::Path::new(dir).join(name)).ok()).and_then(|d| parse_tzif(&d))
}

/// A zone from the time zone database compiled into the program (the only one on Windows).
#[cfg(any(windows, test))]
fn bundled_zone(name: &str) -> Option<Zone> {
    parse_tzif(jiff_tzdb::get(name)?.1)
}

/// Wall clock in a named zone (e.g. "Europe/Berlin") → UTC. Unknown zones are treated as
/// the computer's local time.
pub fn zoned_to_utc(tzid: &str, days: i64, secs_of_day: i64) -> i64 {
    match zone(tzid) {
        Some(z) => wall_to_utc(days * 86_400 + secs_of_day, |u| z.offset(u)),
        None => local_to_utc(days, secs_of_day),
    }
}

/// UTC → (day number, seconds since midnight) on the wall clock of a named zone. An unknown zone
/// is treated as the computer's local time, like [`zoned_to_utc`] does.
pub fn utc_to_zoned(tzid: &str, utc: i64) -> (i64, i64) {
    match zone(tzid) {
        Some(z) => {
            let w = utc + z.offset(utc);
            (w.div_euclid(86_400), w.rem_euclid(86_400))
        }
        None => utc_to_local(utc),
    }
}

/// Do we know the zone (a zoneinfo file of that name, on Windows an entry of the bundled database)?
pub fn zone_known(tzid: &str) -> bool {
    !tzid.trim().is_empty() && zone(tzid).is_some()
}

/// Seconds east of UTC of a named zone at a UTC instant (None for an unknown zone).
pub fn zone_offset(tzid: &str, utc: i64) -> Option<i64> {
    zone(tzid).map(|z| z.offset(utc))
}

/// The changes of a zone's offset in [from, to): (UTC instant, offset before, offset after).
pub fn zone_changes(tzid: &str, from: i64, to: i64) -> Vec<(i64, i64, i64)> {
    zone(tzid).map(|z| z.changes(from, to)).unwrap_or_default()
}

/// The name of the computer's time zone ("Europe/Berlin"), when it can be told and its zone is
/// known: from `TZ`, else from where `/etc/localtime` points. Windows names its zones differently
/// and has neither, so there it is None (unless `TZ` holds an IANA name).
pub fn local_zone_name() -> Option<String> {
    let from_env = std::env::var("TZ").ok().map(|v| v.trim_start_matches(':').to_string()).filter(|v| v.contains('/') && !v.starts_with('/'));
    let from_link = || {
        let target = std::fs::read_link("/etc/localtime").ok()?;
        let s = target.to_string_lossy().into_owned();
        s.rfind("zoneinfo/").map(|p| s[p + "zoneinfo/".len()..].to_string())
    };
    from_env.or_else(from_link).filter(|n| zone_known(n))
}

/// "14:05"
pub fn hhmm(secs_of_day: i64) -> String {
    format!("{:02}:{:02}", secs_of_day / 3600, secs_of_day % 3600 / 60)
}

/// "Thu, 12 Nov 2026"
pub fn long_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{}, {} {} {}", WEEKDAYS[weekday(days) as usize], d, &MONTHS[m as usize - 1][..3], y)
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: CAL-009
    // spec: DEV-007
    #[test]
    fn dev_007_the_fake_now_parses_and_the_clock_is_real_unless_frozen() {
        assert_eq!(parse_fake_now("2026-11-15T10:00:00"), Some(days_from_civil(2026, 11, 15) * 86_400 + 36_000));
        assert_eq!(parse_fake_now("2026-11-15 10:30"), Some(days_from_civil(2026, 11, 15) * 86_400 + 37_800));
        assert_eq!(parse_fake_now("2026-11-15"), Some(days_from_civil(2026, 11, 15) * 86_400));
        for bad in ["", "yesterday", "2026-13-01", "2026-02-30", "2026-11-15T25:00", "2026-11-15T10:00:00:00"] {
            assert_eq!(parse_fake_now(bad), None, "{bad}");
        }
        // the tests never freeze the clock of the process: it is the real one
        assert_eq!(frozen(), None);
        assert!((now() - real_now()).abs() < 5);
    }

    #[test]
    fn civil_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 11, 13), 20_770);
        for d in [-1000, 0, 59, 60, 10_000, 20_770, 40_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(weekday(days_from_civil(2026, 11, 13)), 4, "13 Nov 2026 is a Friday");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(long_date(days_from_civil(2026, 11, 12)), "Thu, 12 Nov 2026");
        assert_eq!(hhmm(14 * 3600 + 5 * 60), "14:05");
    }

    // spec: CAL-009
    #[test]
    fn named_zones_from_zoneinfo() {
        if zone("Europe/Berlin").is_none() {
            return; // no zoneinfo on this system
        }
        // 12:00 in Berlin in winter is 11:00 UTC, in summer 10:00 UTC
        let winter = zoned_to_utc("Europe/Berlin", days_from_civil(2026, 1, 15), 12 * 3600);
        assert_eq!(winter, days_from_civil(2026, 1, 15) * 86_400 + 11 * 3600);
        let summer = zoned_to_utc("Europe/Berlin", days_from_civil(2026, 7, 15), 12 * 3600);
        assert_eq!(summer, days_from_civil(2026, 7, 15) * 86_400 + 10 * 3600);
        let ny = zoned_to_utc("America/New_York", days_from_civil(2026, 1, 15), 9 * 3600);
        assert_eq!(ny, days_from_civil(2026, 1, 15) * 86_400 + 14 * 3600);
        assert!(zone("../etc/passwd").is_none());
    }

    // spec: CAL-009
    #[test]
    fn named_zones_come_from_the_bundled_database_too() {
        // the same answers as the system's zoneinfo gives in `named_zones_from_zoneinfo`
        let berlin = bundled_zone("Europe/Berlin").unwrap();
        let wall = |z: &Zone, y: i32, m: u32, d: u32, h: i64| wall_to_utc(days_from_civil(y, m, d) * 86_400 + h * 3600, |u| z.offset(u));
        assert_eq!(wall(&berlin, 2026, 1, 15, 12), days_from_civil(2026, 1, 15) * 86_400 + 11 * 3600);
        assert_eq!(wall(&berlin, 2026, 7, 15, 12), days_from_civil(2026, 7, 15) * 86_400 + 10 * 3600);
        let ny = bundled_zone("America/New_York").unwrap();
        assert_eq!(wall(&ny, 2026, 1, 15, 9), days_from_civil(2026, 1, 15) * 86_400 + 14 * 3600);
        assert!(bundled_zone("Europe/Nowhere").is_none());
        assert!(bundled_zone("../etc/passwd").is_none());
        assert!(bundled_zone("").is_none());
    }

    // spec: CAL-018
    #[cfg(windows)]
    #[test]
    fn windows_knows_named_zones_without_any_zoneinfo_files() {
        assert!(zone_known("Europe/Berlin") && zone_known("\"America/New_York\""));
        assert_eq!(zone_offset("Europe/Berlin", days_from_civil(2030, 7, 15) * 86_400), Some(7200));
        assert!(!zone_known("Mars/Olympus_Mons") && !zone_known("../etc/passwd"));
    }

    /// Changes of the years 2030 and 2045, from a full (not slim) database: (UTC instant, before, after).
    type Changes = [(i64, i64, i64); 2];
    const CHANGES: [(&str, Changes, Changes); 4] = [
        ("Europe/Berlin", [(1_901_149_200, 3600, 7200), (1_919_293_200, 7200, 3600)], [(2_374_102_800, 3600, 7200), (2_392_851_600, 7200, 3600)]),
        ("America/New_York", [(1_899_356_400, -18_000, -14_400), (1_919_916_000, -14_400, -18_000)], [(2_372_914_800, -18_000, -14_400), (2_393_474_400, -14_400, -18_000)]),
        ("Australia/Sydney", [(1_901_721_600, 39_600, 36_000), (1_917_446_400, 36_000, 39_600)], [(2_374_675_200, 39_600, 36_000), (2_390_400_000, 36_000, 39_600)]),
        // a rule with a negative time: M3.5.0/-1
        ("America/Nuuk", [(1_901_149_200, -7200, -3600), (1_919_293_200, -3600, -7200)], [(2_374_102_800, -7200, -3600), (2_392_851_600, -3600, -7200)]),
    ];

    fn year(y: i32) -> (i64, i64) {
        (days_from_civil(y, 1, 1) * 86_400, days_from_civil(y + 1, 1, 1) * 86_400)
    }

    // spec: CAL-023
    #[test]
    fn the_bundled_data_is_slim_and_the_footer_rule_covers_the_years_after_its_last_transition() {
        let berlin = bundled_zone("Europe/Berlin").unwrap();
        // the trap: the table ends in 1996, so without the rule every later time has the offset of its last entry
        assert!(berlin.transitions.last().unwrap().0 < year(2030).0, "the bundled database lists no transition from 2030 on");
        assert!(berlin.footer.is_some());
        let july = days_from_civil(2030, 7, 15) * 86_400 + 43_200;
        assert_eq!(berlin.offset(july), 7200);
        let table_only = Zone { transitions: berlin.transitions.clone(), initial: berlin.initial, footer: None };
        let january = days_from_civil(2030, 1, 15) * 86_400 + 43_200;
        assert_eq!((berlin.offset(january), table_only.offset(january)), (3600, 7200), "the table alone ends in the summer of 1996 and keeps that offset for ever");
        assert!(table_only.changes(year(2030).0, year(2030).1).is_empty());

        for (name, y2030, y2045) in CHANGES {
            let z = bundled_zone(name).unwrap();
            for (y, expected) in [(2030, y2030), (2045, y2045)] {
                let (from, to) = year(y);
                assert_eq!(z.changes(from, to), expected, "{name} {y}");
                // the offset follows the changes: one second before and at each change
                for (at, before, after) in expected {
                    assert_eq!((z.offset(at - 1), z.offset(at)), (before, after), "{name} {at}");
                }
            }
            // a range that cuts the year in two sees one change each
            let (from, to) = year(2030);
            assert_eq!(z.changes(from, y2030[1].0), vec![y2030[0]], "{name}");
            assert_eq!(z.changes(y2030[1].0, to), vec![y2030[1]], "{name}");
        }
        // no summer time: Tokyo has no change and the offset of the rule
        let tokyo = bundled_zone("Asia/Tokyo").unwrap();
        assert!(tokyo.changes(year(2030).0, year(2045).1).is_empty());
        assert_eq!(tokyo.offset(july), 32_400);
        // Dublin: the footer has standard time +1 and "summer time" 0 in winter (a negative saving)
        let dublin = bundled_zone("Europe/Dublin").unwrap();
        assert_eq!((dublin.offset(july), dublin.offset(days_from_civil(2030, 1, 15) * 86_400)), (3600, 0));
        assert_eq!(dublin.changes(year(2030).0, year(2030).1), vec![(1_901_149_200, 0, 3600), (1_919_293_200, 3600, 0)]);
        // a zone without any transition (UTC) has the rule only
        assert_eq!(bundled_zone("UTC").unwrap().offset(july), 0);
    }

    // spec: CAL-023
    #[test]
    fn the_public_functions_use_the_footer_rule_for_years_after_the_table() {
        if zone("Europe/Berlin").is_none() {
            return; // no zoneinfo on this system
        }
        // 2045-07-15 12:00 in Berlin is 10:00 UTC, on a system with a slim database too
        let d = days_from_civil(2045, 7, 15);
        assert_eq!(zoned_to_utc("Europe/Berlin", d, 12 * 3600), d * 86_400 + 10 * 3600);
        assert_eq!(utc_to_zoned("Europe/Berlin", d * 86_400 + 10 * 3600), (d, 12 * 3600));
        assert_eq!(zone_offset("Europe/Berlin", d * 86_400), Some(7200));
        assert_eq!(zone_changes("Europe/Berlin", year(2045).0, year(2045).1), CHANGES[0].2.to_vec());
    }

    /// A TZif version 2 file: the empty 32-bit part, then the 64-bit part with these transitions
    /// (instant, index into `types`), these offsets and the footer.
    fn tzif(transitions: &[(i64, u8)], types: &[i32], footer: &str) -> Vec<u8> {
        let header = |timecnt: usize, typecnt: usize, charcnt: usize| {
            let mut h = b"TZif2".to_vec();
            h.extend([0; 15]);
            for n in [0, 0, 0, timecnt, typecnt, charcnt] {
                h.extend((n as u32).to_be_bytes());
            }
            h
        };
        let mut out = header(0, 0, 0);
        out.extend(header(transitions.len(), types.len(), 1));
        out.extend(transitions.iter().flat_map(|t| t.0.to_be_bytes()));
        out.extend(transitions.iter().map(|t| t.1));
        out.extend(types.iter().flat_map(|t| t.to_be_bytes().into_iter().chain([0, 0])));
        out.push(0);
        out.extend(format!("\n{footer}\n").into_bytes());
        out
    }

    // spec: CAL-023
    #[test]
    fn the_footer_of_a_tzif_file_is_read_and_a_broken_one_keeps_the_last_offset() {
        let at = |y, m, d| days_from_civil(y, m, d) * 86_400;
        let table = [(at(2000, 7, 1), 1u8)];
        // summer time rule in the footer after a table that ends in 2000
        let z = parse_tzif(&tzif(&table, &[3600, 7200], "CET-1CEST,M3.5.0,M10.5.0/3")).unwrap();
        assert_eq!((z.offset(at(1999, 1, 1)), z.offset(at(2000, 7, 1))), (3600, 7200), "the table decides before its end");
        assert_eq!((z.offset(at(2030, 1, 15)), z.offset(at(2030, 7, 15))), (3600, 7200));
        assert_eq!(z.changes(year(2030).0, year(2030).1), CHANGES[0].1.to_vec());
        assert!(z.changes(year(1999).0, year(2000).0).is_empty());
        // no rule, an unreadable one and none at all: the last offset stays
        for footer in ["", "<>", "CET-1CEST,M3.5.0", "garbage"] {
            let z = parse_tzif(&tzif(&table, &[3600, 7200], footer)).unwrap();
            assert!(z.footer.is_none(), "{footer:?}");
            assert_eq!(z.offset(at(2030, 1, 15)), 7200, "{footer:?}");
            assert!(z.changes(year(2030).0, year(2030).1).is_empty());
        }
        let mut no_newline = tzif(&table, &[3600, 7200], "UTC0");
        no_newline.pop();
        assert!(parse_tzif(&no_newline).unwrap().footer.is_none());
        // a zone with no transitions at all is its rule
        let fixed = parse_tzif(&tzif(&[], &[0], "<+03>-3")).unwrap();
        assert_eq!(fixed.offset(at(2030, 1, 1)), 10_800);
    }

    // spec: CAL-009
    #[cfg(not(windows))]
    #[test]
    fn the_bundled_database_and_the_system_zoneinfo_agree() {
        if system_zone("Europe/Berlin").is_none() {
            return; // no zoneinfo on this system to compare with
        }
        for name in ["Europe/Berlin", "America/New_York", "Australia/Sydney", "Asia/Tokyo", "Europe/Dublin", "America/Nuuk", "Asia/Kolkata", "Pacific/Auckland", "America/Sao_Paulo", "UTC"] {
            let (system, bundled) = (system_zone(name).unwrap(), bundled_zone(name).unwrap());
            // every 6 hours from 1970 to the end of 2025, and every change of 2010 to 2025. Only the
            // past: the two databases can be of different releases, and a new rule changes the years to
            // come (those are checked against fixed values above)
            let mut utc = 0;
            while utc < year(2026).0 {
                assert_eq!(system.offset(utc), bundled.offset(utc), "{name} at {utc}");
                utc += 6 * 3600;
            }
            assert_eq!(system.changes(year(2010).0, year(2026).0), bundled.changes(year(2010).0, year(2026).0), "{name}");
        }
    }

    // spec: CAL-009
    #[test]
    fn local_round_trip() {
        let d = days_from_civil(2026, 3, 10);
        let utc = local_to_utc(d, 9 * 3600 + 30 * 60);
        assert_eq!(utc_to_local(utc), (d, 9 * 3600 + 30 * 60));
    }

    // spec: CAL-018
    #[test]
    fn the_local_offset_is_a_real_zone_offset_and_local_time_round_trips() {
        for utc in [0, 1_000_000_000, days_from_civil(2026, 1, 15) * 86_400 + 43_200, days_from_civil(2026, 7, 15) * 86_400 + 43_200, days_from_civil(2026, 11, 15) * 86_400] {
            let off = local_offset(utc);
            assert!(off.abs() <= 14 * 3600 && off % 900 == 0, "{off} at {utc}");
            let (day, sod) = utc_to_local(utc);
            assert_eq!(local_to_utc(day, sod), utc, "{utc}");
        }
        // the same moment is one local calendar day at most away from its UTC day
        let utc = days_from_civil(2026, 11, 15) * 86_400 + 12 * 3600;
        assert!((utc_to_local(utc).0 - utc.div_euclid(86_400)).abs() <= 1);
        // the range the Windows call cannot take falls back to no offset instead of failing
        assert!(local_offset(-20_000_000_000).abs() <= 14 * 3600);
    }
}
