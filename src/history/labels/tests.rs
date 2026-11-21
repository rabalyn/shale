use super::*;
use crate::calendar::time::{days_from_civil, local_to_utc};
use crate::gitsync::history::Meta;
use std::path::PathBuf;

/// A local time as a unix time.
fn at(y: i32, m: u32, d: u32, h: i64, min: i64) -> i64 {
    local_to_utc(days_from_civil(y, m, d), h * 3600 + min * 60)
}

fn version(commit: &str, time: i64, reason: Option<Reason>, device: Option<&str>, path: &str) -> Version {
    Version { commit: commit.into(), time, meta: Meta { subject: String::new(), reason, device: device.map(str::to_string), paths: vec![], restored: None }, path: PathBuf::from(path), lines: Some((1, 0)), binary: false }
}

// spec: HIST-004
#[test]
fn hist_004_days_are_today_yesterday_or_a_date_and_the_year_only_when_it_is_not_this_year() {
    let now = at(2026, 11, 21, 15, 4);
    assert_eq!(day_label(at(2026, 11, 21, 0, 1), now), "Today");
    assert_eq!(day_label(at(2026, 11, 20, 23, 59), now), "Yesterday");
    assert_eq!(day_label(at(2026, 11, 12, 9, 0), now), "12 Nov");
    assert_eq!(day_label(at(2025, 12, 31, 9, 0), now), "31 Dec 2025");
    assert_eq!(day_label(at(2026, 1, 1, 9, 0), now), "1 Jan");
}

// spec: HIST-004
#[test]
fn hist_004_times_are_relative_within_the_hour_and_a_clock_time_after() {
    let now = at(2026, 11, 21, 15, 4);
    assert_eq!(time_label(now - 5, now), "just now");
    assert_eq!(time_label(now - 59, now), "just now");
    assert_eq!(time_label(now - 60, now), "1 min ago");
    assert_eq!(time_label(now - 5 * 60, now), "5 min ago");
    assert_eq!(time_label(now - 3599, now), "59 min ago");
    assert_eq!(time_label(at(2026, 11, 21, 11, 5), now), "11:05");
    assert_eq!(time_label(at(2026, 11, 20, 18, 40), now), "18:40");
    assert_eq!(day_time(at(2026, 11, 21, 11, 5), now), "11:05");
    assert_eq!(day_time(at(2026, 11, 20, 18, 40), now), "Yesterday 18:40");
    assert_eq!(day_time(at(2026, 11, 12, 9, 0), now), "12 Nov 09:00");
    assert_eq!(full_time(at(2026, 11, 21, 15, 4)), "2026-11-21 15:04");
}

// spec: HIST-004
#[test]
fn hist_004_versions_are_grouped_by_day_newest_first() {
    let now = at(2026, 11, 21, 15, 4);
    let vs = [at(2026, 11, 21, 14, 32), at(2026, 11, 21, 11, 5), at(2026, 11, 20, 18, 40), at(2026, 11, 20, 9, 12), at(2026, 11, 12, 8, 0)].map(|t| version("c", t, Some(Reason::Save), None, "N.md"));
    assert_eq!(group_by_day(&vs, now), [("Today".to_string(), vec![0, 1]), ("Yesterday".to_string(), vec![2, 3]), ("12 Nov".to_string(), vec![4])]);
    assert!(group_by_day(&[], now).is_empty());
}

// spec: HIST-005, HIST-017
#[test]
fn hist_005_every_reason_has_its_plain_words_and_none_is_git_talk() {
    let now = at(2026, 11, 21, 15, 4);
    let make = |r: Option<Reason>, d: Option<&str>| vec![version("a", now, r, d, "New.md")];
    let label = |r: Option<Reason>, d: Option<&str>| source_label(&make(r, d), 0, "desk", now);
    for r in [Reason::Save, Reason::Idle, Reason::Close] {
        assert_eq!(label(Some(r), Some("desk")), "edited here");
        assert_eq!(label(Some(r), None), "edited here");
        assert_eq!(label(Some(r), Some("laptop")), "edited on laptop");
    }
    assert_eq!(label(Some(Reason::Initial), Some("desk")), "first version kept");
    assert_eq!(label(Some(Reason::Sync), Some("laptop")), "synced from laptop");
    assert_eq!(label(Some(Reason::Sync), None), "synced");
    assert_eq!(label(Some(Reason::Merge), Some("laptop")), "merged after a conflict");
    assert_eq!(label(Some(Reason::Delete), None), "deleted");
    assert_eq!(label(None, None), "changed outside Shale", "a commit without trailers");
    // a restore names the time of the version it went back to, if that version is in the list
    let mut vs = vec![version("new", now, Some(Reason::Restore), Some("desk"), "N.md"), version("old", at(2026, 11, 17, 9, 0), Some(Reason::Save), None, "N.md")];
    vs[0].meta.restored = Some("old".into());
    assert_eq!(source_label(&vs, 0, "desk", now), "restored version of 17 Nov 09:00");
    vs[0].meta.restored = Some("gone".into());
    assert_eq!(source_label(&vs, 0, "desk", now), "restored an earlier version");
    vs[0].meta.restored = None;
    assert_eq!(source_label(&vs, 0, "desk", now), "restored an earlier version");
    // a rename names the old name: from the older version, or from the trailers
    let mut vs = vec![version("new", now, Some(Reason::Rename), None, "Trips/New.md"), version("old", at(2026, 11, 17, 9, 0), Some(Reason::Save), None, "Old.md")];
    assert_eq!(source_label(&vs, 0, "desk", now), "renamed from Old.md");
    vs.pop();
    vs[0].meta.paths = vec!["Before.md".into(), "Trips/New.md".into()];
    assert_eq!(source_label(&vs, 0, "desk", now), "renamed from Before.md");
    vs[0].meta.paths.clear();
    assert_eq!(source_label(&vs, 0, "desk", now), "renamed");
    // nothing in a label talks like a git tool
    for r in [Reason::Save, Reason::Idle, Reason::Close, Reason::Initial, Reason::Sync, Reason::Restore, Reason::Merge, Reason::Rename, Reason::Delete] {
        let l = label(Some(r), Some("laptop")).to_lowercase();
        for bad in ["commit", "head", "diff", "revision", "snapshot"] {
            assert!(!l.contains(bad), "{l}");
        }
    }
}

// spec: HIST-006
#[test]
fn hist_006_the_size_of_a_change_has_a_hover_a_text_and_five_ticks() {
    assert_eq!(size_hover(Some((12, 3))), "12 lines added, 3 removed");
    assert_eq!(size_hover(Some((1, 0))), "1 line added");
    assert_eq!(size_hover(Some((0, 4))), "4 lines removed");
    assert_eq!(size_hover(Some((1, 1))), "1 line added, 1 removed");
    assert_eq!(size_hover(Some((0, 0))), "no lines changed");
    assert_eq!(size_hover(None), "changed");
    assert_eq!(size_text(Some((12, 3))), "+12 \u{2212}3");
    assert_eq!(size_text(Some((1, 0))), "+1");
    assert_eq!(size_text(Some((0, 2))), "\u{2212}2");
    assert_eq!(size_text(Some((0, 0))), "");
    assert_eq!(size_text(None), "changed");
    assert_eq!(ticks(0, 0), (0, 0));
    assert_eq!(ticks(40, 0), (5, 0));
    assert_eq!(ticks(0, 1), (0, 5));
    assert_eq!(ticks(12, 3), (4, 1));
    assert_eq!(ticks(1, 1000), (1, 4), "a side that is not 0 keeps one tick");
    assert_eq!(ticks(1000, 1), (4, 1));
    assert_eq!(ticks(5, 5).0 + ticks(5, 5).1, 5);
}
