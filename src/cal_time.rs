//! Date and time helpers for the calendar, without a date library:
//! civil dates ↔ day numbers, local time through the C library, and named time zones (TZID)
//! read from the system's zoneinfo files (TZif).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

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
    let _ = utc;
    0
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

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn wall_to_utc(wall: i64, offset_at: impl Fn(i64) -> i64) -> i64 {
    let mut utc = wall - offset_at(wall);
    for _ in 0..2 {
        utc = wall - offset_at(utc);
    }
    utc
}

/// A zone read from a TZif file: transition times and the offset in effect after each.
struct Zone {
    transitions: Vec<(i64, i64)>,
    initial: i64,
}

impl Zone {
    fn offset(&self, utc: i64) -> i64 {
        match self.transitions.binary_search_by(|t| t.0.cmp(&utc)) {
            Ok(i) => self.transitions[i].1,
            Err(0) => self.initial,
            Err(i) => self.transitions[i - 1].1,
        }
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
    let [_, _, _, timecnt, typecnt, _] = counts(base)?;
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
    Some(Zone { transitions, initial: *types.first().unwrap_or(&0) })
}

fn zone(tzid: &str) -> Option<std::sync::Arc<Zone>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<std::sync::Arc<Zone>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(z) = cache.lock().unwrap().get(tzid) {
        return z.clone();
    }
    let clean = tzid.trim_matches('"').trim_start_matches('/');
    let z = if clean.contains("..") {
        None
    } else {
        ["/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"].iter().find_map(|dir| crate::fsutil::read(std::path::Path::new(dir).join(clean)).ok()).and_then(|d| parse_tzif(&d)).map(std::sync::Arc::new)
    };
    cache.lock().unwrap().insert(tzid.to_string(), z.clone());
    z
}

/// Wall clock in a named zone (e.g. "Europe/Berlin") → UTC. Unknown zones are treated as
/// the computer's local time.
pub fn zoned_to_utc(tzid: &str, days: i64, secs_of_day: i64) -> i64 {
    match zone(tzid) {
        Some(z) => wall_to_utc(days * 86_400 + secs_of_day, |u| z.offset(u)),
        None => local_to_utc(days, secs_of_day),
    }
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
    fn local_round_trip() {
        let d = days_from_civil(2026, 3, 10);
        let utc = local_to_utc(d, 9 * 3600 + 30 * 60);
        assert_eq!(utc_to_local(utc), (d, 9 * 3600 + 30 * 60));
    }
}
