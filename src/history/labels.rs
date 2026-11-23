//! The words of the history list (HIST-004…HIST-006): days, times, where a version came from and
//! how big a change was. Every function takes the time "now" and the device's name as arguments, so
//! it can be tested.

use crate::calendar::time::{civil_from_days, hhmm, utc_to_local, MONTHS};
use crate::gitsync::history::{Reason, Version};
use std::path::Path;

/// "12 Nov", or "12 Nov 2025" when it is not this year.
fn date_text(day: i64, today: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    let name = &MONTHS[m as usize - 1][..3];
    if civil_from_days(today).0 == y {
        format!("{d} {name}")
    } else {
        format!("{d} {name} {y}")
    }
}

/// The heading of a day: Today, Yesterday, or the date (HIST-004).
pub fn day_label(ts: i64, now: i64) -> String {
    let (day, today) = (utc_to_local(ts).0, utc_to_local(now).0);
    match today - day {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        _ => date_text(day, today),
    }
}

/// The time of a row: "just now", "5 min ago" or the time of day (HIST-004).
pub fn time_label(ts: i64, now: i64) -> String {
    match now - ts {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{} min ago", s / 60),
        _ => hhmm(utc_to_local(ts).1),
    }
}

/// The time with its day, for a tooltip and for "Changes since …": "14:32", "Yesterday 18:40", "12 Nov 09:00".
pub fn day_time(ts: i64, now: i64) -> String {
    let clock = hhmm(utc_to_local(ts).1);
    match day_label(ts, now).as_str() {
        "Today" => clock,
        day => format!("{day} {clock}"),
    }
}

/// The full date and time for a hover: "2026-11-21 15:04".
pub fn full_time(ts: i64) -> String {
    let (day, secs) = utc_to_local(ts);
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02} {}", hhmm(secs))
}

/// The versions grouped by day, newest first: the heading and the indexes into `versions`.
pub fn group_by_day(versions: &[Version], now: i64) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = vec![];
    for (i, v) in versions.iter().enumerate() {
        let label = day_label(v.time, now);
        match groups.last_mut() {
            Some((l, idx)) if *l == label => idx.push(i),
            _ => groups.push((label, vec![i])),
        }
    }
    groups
}

/// Where version `i` of the list came from, in plain words (HIST-005). `this_device` is this
/// computer's name: an edit from another device says which.
pub fn source_label(versions: &[Version], i: usize, this_device: &str, now: i64) -> String {
    let v = &versions[i];
    let device = v.meta.device.as_deref().filter(|d| !d.is_empty());
    match v.meta.reason {
        Some(Reason::Save | Reason::Idle | Reason::Close) => match device {
            Some(d) if d != this_device => format!("edited on {d}"),
            _ => "edited here".to_string(),
        },
        Some(Reason::Initial) => "first version kept".to_string(),
        Some(Reason::Sync) => match device {
            Some(d) => format!("synced from {d}"),
            None => "synced".to_string(),
        },
        Some(Reason::Merge) => "merged after a conflict".to_string(),
        Some(Reason::Restore) => match v.meta.restored.as_deref().and_then(|c| versions.iter().find(|o| o.commit == c)) {
            Some(o) => format!("restored version of {}", day_time(o.time, now)),
            None => "restored an earlier version".to_string(),
        },
        Some(Reason::Rename) => {
            let older = versions.get(i + 1).map(|o| o.path.as_path()).filter(|p| *p != v.path);
            let named = v.meta.paths.first().map(Path::new).filter(|p| *p != v.path);
            match older.or(named).and_then(|p| p.file_name()) {
                Some(n) => format!("renamed from {}", n.to_string_lossy()),
                None => "renamed".to_string(),
            }
        }
        Some(Reason::Delete) => "deleted".to_string(),
        Some(Reason::Import) => "imported".to_string(),
        None => "changed outside Shale".to_string(),
    }
}

/// "12 lines added, 3 removed" for the hover of a row (HIST-006); "changed" for a binary version.
pub fn size_hover(lines: Option<(u32, u32)>) -> String {
    let n = |k: u32| if k == 1 { "1 line".to_string() } else { format!("{k} lines") };
    match lines {
        None => "changed".to_string(),
        Some((0, 0)) => "no lines changed".to_string(),
        Some((a, 0)) => format!("{} added", n(a)),
        Some((0, r)) => format!("{} removed", n(r)),
        Some((a, r)) => format!("{} added, {r} removed", n(a)),
    }
}

/// The text beside the bar: "+12 −3".
pub fn size_text(lines: Option<(u32, u32)>) -> String {
    match lines {
        None => "changed".to_string(),
        Some((a, r)) => [(a, '+'), (r, '\u{2212}')].iter().filter(|(n, _)| *n > 0).map(|(n, c)| format!("{c}{n}")).collect::<Vec<_>>().join(" "),
    }
}

/// How many of the five ticks are green and how many red (HIST-006): in proportion, at least one for
/// a side that is not 0.
pub fn ticks(added: u32, removed: u32) -> (u8, u8) {
    const TICKS: u32 = 5;
    let total = added + removed;
    if total == 0 {
        return (0, 0);
    }
    let green = (added * TICKS + total / 2) / total;
    let green = match (added, removed) {
        (0, _) => 0,
        (_, 0) => TICKS,
        _ => green.clamp(1, TICKS - 1),
    };
    (green as u8, if removed == 0 { 0 } else { (TICKS - green) as u8 })
}

#[cfg(test)]
mod tests;
