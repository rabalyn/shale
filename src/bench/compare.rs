//! Comparing two results (BUILD-030, BUILD-036): `shale --bench-compare x.json y.json` says, per
//! scenario, whether Y (the newer run) is faster, slower or a regression against X, writes
//! `compare.md` and `compare.json` and ends with exit code 0 (no regression), 1 (a regression)
//! or 2 (an error), so a release script or an agent can act on it.
//!
//! What a verdict trusts: allocation counts and peak bytes are exact (the same on every run, busy
//! computer or not), so a change in them is a verdict on its own. A time moves with the load of
//! the computer, so a time difference counts only when it is above the threshold **and** the spread
//! of X and the spread of Y do not overlap (the spread: the range of the medians of the merged
//! runs, `--bench-merge`; for a single run the range from the fastest to the 95th percentile
//! iteration). A difference inside the spread is `noisy`, not a regression. A scenario whose
//! `version` differs between X and Y (or is named by `--changed`) is `changed`: no verdict.

use super::runner::{read_report, Report, ScenarioResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct Thresholds {
    /// a median time this many percent above X is a regression (when the spreads do not overlap)
    pub time_pct: f64,
    /// a peak above X by this many percent is a regression
    pub bytes_pct: f64,
    /// a count of allocations above X by this many percent is a regression
    pub allocs_pct: f64,
}

/// Time and peak differences within this many percent are noise: `ok`.
pub const NOISE_PCT: f64 = 5.0;
/// Allocation counts are exact: a difference within this many percent is `ok`.
pub const ALLOCS_NOISE_PCT: f64 = 2.0;
/// A time difference below this is noise whatever its percentage (a scenario of a few microseconds).
pub const TIME_FLOOR_NS: u64 = 20_000;
/// A peak difference below this is noise whatever its percentage.
pub const BYTES_FLOOR: u64 = 64 * 1024;
/// A difference of fewer allocations than this is noise whatever its percentage.
pub const ALLOCS_FLOOR: u64 = 500;
/// A time regression below this many percent on a busy computer has `confidence` `low`.
pub const LOW_CONFIDENCE_BELOW_PCT: f64 = 30.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Side {
    pub commit: String,
    pub describe: String,
    pub date: String,
    /// the highest load average of the computer at the end of a run, when it was written
    #[serde(default)]
    pub load: Option<f64>,
    pub cores: usize,
    /// how many runs the result merges
    #[serde(default)]
    pub repeats: u32,
}

/// One scenario of the comparison. `status` is `both`, `changed` (its version differs: no
/// verdicts), `added` (only in Y) or `removed` (only in X).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub status: String,
    pub x_median_ns: Option<u64>,
    pub y_median_ns: Option<u64>,
    pub time_delta_pct: Option<f64>,
    /// `ok`, `faster`, `slower`, `noisy` (beyond the noise band but inside the spread), `REGRESSION`, `changed` (`-` for added and removed)
    pub time_verdict: String,
    /// the spread of each side's time: the fastest and the slowest of the runs' medians
    pub x_range_ns: Option<[u64; 2]>,
    pub y_range_ns: Option<[u64; 2]>,
    pub x_allocs: Option<u64>,
    pub y_allocs: Option<u64>,
    pub allocs_delta_pct: Option<f64>,
    /// `ok`, `fewer`, `more`, `REGRESSION`, `tradeoff`, `changed`
    pub allocs_verdict: String,
    pub x_peak_bytes: Option<u64>,
    pub y_peak_bytes: Option<u64>,
    pub bytes_delta_pct: Option<f64>,
    /// `ok`, `smaller`, `larger`, `REGRESSION`, `tradeoff`, `changed`
    pub bytes_verdict: String,
    pub x_version: Option<u32>,
    pub y_version: Option<u32>,
}

/// A regression or an improvement of one metric of one scenario.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    /// `time`, `allocs` or `peak_bytes`
    pub metric: String,
    pub x: u64,
    pub y: u64,
    pub delta_pct: f64,
    /// `high`, or `low` for a time regression under 30 % on a busy computer: run it again with `--filter` on a quiet one
    pub confidence: String,
}

/// A scenario that has no verdict because its meaning changed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Changed {
    pub id: String,
    /// `version` (the two results carry different scenario versions) or `--changed` (named on the command line)
    pub reason: String,
    pub x_version: Option<u32>,
    pub y_version: Option<u32>,
}

/// A scenario one side could not run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SkippedRow {
    pub id: String,
    /// `x`, `y` or `both`
    pub side: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub compared: usize,
    pub faster: usize,
    pub slower: usize,
    pub regressions: usize,
    pub noisy: usize,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Comparison {
    pub schema: u32,
    /// `ok`, `faster` (no regression, at least one improvement), `regression` or `error`
    pub verdict: String,
    /// 0 (`ok`, `faster`), 1 (`regression`), 2 (`error`)
    pub exit_code: i32,
    pub x: Side,
    pub y: Side,
    /// a computer busy during a run: times are larger and less steady (the counts are not)
    pub busy: bool,
    pub threshold_time_pct: f64,
    pub threshold_bytes_pct: f64,
    pub threshold_allocs_pct: f64,
    pub rows: Vec<Row>,
    /// the worst first
    pub regressions: Vec<Change>,
    /// the best first
    pub improvements: Vec<Change>,
    /// allocations or peak above the threshold in a scenario whose time is faster by more than the threshold (a cache, a tree): reported, not a regression
    #[serde(default)]
    pub tradeoffs: Vec<Change>,
    /// scenarios whose time moved more than the threshold but inside the spread: run them again with `--filter`
    pub noisy: Vec<String>,
    pub changed: Vec<Changed>,
    pub skipped: Vec<SkippedRow>,
    pub summary: Summary,
}

/// What `compare.json` holds when the comparison could not be made.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Failure {
    pub schema: u32,
    pub verdict: String,
    pub exit_code: i32,
    pub error: String,
}

fn delta(x: u64, y: u64) -> f64 {
    if x == 0 {
        return if y == 0 { 0.0 } else { 100.0 };
    }
    (y as f64 - x as f64) / x as f64 * 100.0
}

/// The spread of one side's time: the range of its runs' medians, or for one run the fastest
/// iteration to the 95th percentile one.
fn range(s: &ScenarioResult) -> [u64; 2] {
    if s.run_medians_ns.len() >= 2 {
        [s.run_medians_ns.iter().copied().min().unwrap_or(0), s.run_medians_ns.iter().copied().max().unwrap_or(0)]
    } else {
        [s.min_ns.min(s.median_ns), s.p95_ns.max(s.median_ns)]
    }
}

/// The verdict of an exact metric (allocation count, peak bytes): the percentage and the absolute floor decide.
fn exact(x: u64, y: u64, limit_pct: f64, noise_pct: f64, floor: u64, words: (&'static str, &'static str)) -> (f64, &'static str) {
    let d = delta(x, y);
    let big = x.abs_diff(y) >= floor;
    let v = if d > limit_pct && big {
        "REGRESSION"
    } else if d > noise_pct && big {
        words.0
    } else if d < -noise_pct && big {
        words.1
    } else {
        "ok"
    };
    (d, v)
}

/// The verdict of the time: the percentage, the floor, and the spreads (they must not overlap).
fn timed(a: &ScenarioResult, b: &ScenarioResult, limit_pct: f64) -> (f64, &'static str) {
    let d = delta(a.median_ns, b.median_ns);
    if a.median_ns.abs_diff(b.median_ns) < TIME_FLOOR_NS || d.abs() <= NOISE_PCT {
        return (d, "ok");
    }
    let (x, y) = (range(a), range(b));
    let v = if d > 0.0 {
        if y[0] <= x[1] {
            "noisy"
        } else if d > limit_pct {
            "REGRESSION"
        } else {
            "slower"
        }
    } else if y[1] >= x[0] {
        "noisy"
    } else {
        "faster"
    };
    (d, v)
}

/// The rows of two results, the changes worth acting on, and the verdict. `forced_changed` names
/// scenarios to treat as changed whatever their versions say (for results older than the field).
pub fn compare(x: &Report, y: &Report, t: Thresholds, forced_changed: &[String]) -> Comparison {
    let xs: BTreeMap<&str, &ScenarioResult> = x.scenarios.iter().map(|s| (s.id.as_str(), s)).collect();
    let ys: BTreeMap<&str, &ScenarioResult> = y.scenarios.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut ids: Vec<&str> = xs.keys().chain(ys.keys()).copied().collect();
    // the order of Y (the newer run), then what only X has
    ids.sort_by_key(|id| (y.scenarios.iter().position(|s| s.id == *id).unwrap_or(usize::MAX), *id));
    ids.dedup();
    let busy_side = |r: &Report| r.host.load.is_some_and(|l| l > r.host.cores as f64 / 2.0);
    let busy = busy_side(x) || busy_side(y);
    let (mut rows, mut regressions, mut improvements, mut noisy, mut changed, mut tradeoffs) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut sum = Summary { compared: 0, faster: 0, slower: 0, regressions: 0, noisy: 0, changed: 0, added: 0, removed: 0 };
    for id in ids {
        let (a, b) = (xs.get(id), ys.get(id));
        let mut row = Row {
            id: id.into(),
            status: "both".into(),
            x_median_ns: a.map(|s| s.median_ns),
            y_median_ns: b.map(|s| s.median_ns),
            time_delta_pct: None,
            time_verdict: "-".into(),
            x_range_ns: a.map(|s| range(s)),
            y_range_ns: b.map(|s| range(s)),
            x_allocs: a.map(|s| s.allocs),
            y_allocs: b.map(|s| s.allocs),
            allocs_delta_pct: None,
            allocs_verdict: "-".into(),
            x_peak_bytes: a.map(|s| s.peak_bytes),
            y_peak_bytes: b.map(|s| s.peak_bytes),
            bytes_delta_pct: None,
            bytes_verdict: "-".into(),
            x_version: a.map(|s| s.version),
            y_version: b.map(|s| s.version),
        };
        let (Some(a), Some(b)) = (a, b) else {
            row.status = if a.is_some() { "removed" } else { "added" }.into();
            if a.is_some() {
                sum.removed += 1;
            } else {
                sum.added += 1;
            }
            rows.push(row);
            continue;
        };
        sum.compared += 1;
        let reason = if forced_changed.iter().any(|c| c == id) {
            Some("--changed")
        } else if a.version != b.version {
            Some("version")
        } else {
            None
        };
        if let Some(reason) = reason {
            row.status = "changed".into();
            (row.time_verdict, row.allocs_verdict, row.bytes_verdict) = ("changed".into(), "changed".into(), "changed".into());
            changed.push(Changed { id: id.into(), reason: reason.into(), x_version: Some(a.version), y_version: Some(b.version) });
            sum.changed += 1;
            rows.push(row);
            continue;
        }
        let (td, tv) = timed(a, b, t.time_pct);
        let (ad, av) = exact(a.allocs, b.allocs, t.allocs_pct, ALLOCS_NOISE_PCT, ALLOCS_FLOOR, ("more", "fewer"));
        let (bd, bv) = exact(a.peak_bytes, b.peak_bytes, t.bytes_pct, NOISE_PCT, BYTES_FLOOR, ("larger", "smaller"));
        // more memory for a time that is clearly better is a trade, listed but not a regression
        let traded = tv == "faster" && td < -t.time_pct;
        let trade = |v: &'static str| if traded && v == "REGRESSION" { "tradeoff" } else { v };
        let (av, bv) = (trade(av), trade(bv));
        let metrics = [("time", tv, a.median_ns, b.median_ns, td), ("allocs", av, a.allocs, b.allocs, ad), ("peak_bytes", bv, a.peak_bytes, b.peak_bytes, bd)];
        for (metric, v, xv, yv, d) in metrics {
            let change = |confidence: &str| Change { id: id.into(), metric: metric.into(), x: xv, y: yv, delta_pct: d, confidence: confidence.into() };
            match v {
                "REGRESSION" => {
                    let low = metric == "time" && busy && d < LOW_CONFIDENCE_BELOW_PCT;
                    regressions.push(change(if low { "low" } else { "high" }));
                }
                "tradeoff" => tradeoffs.push(change("high")),
                "faster" | "fewer" | "smaller" => improvements.push(change("high")),
                "noisy" if d.abs() > t.time_pct => noisy.push(id.to_string()),
                _ => {}
            }
        }
        let any = |w: &[&str]| [tv, av, bv].iter().any(|v| w.contains(v));
        if any(&["REGRESSION"]) {
            sum.regressions += 1;
        } else if any(&["slower", "more", "larger"]) {
            sum.slower += 1;
        } else if any(&["faster", "fewer", "smaller"]) {
            sum.faster += 1;
        }
        if tv == "noisy" {
            sum.noisy += 1;
        }
        row.time_delta_pct = Some(td);
        row.time_verdict = tv.into();
        row.allocs_delta_pct = Some(ad);
        row.allocs_verdict = av.into();
        row.bytes_delta_pct = Some(bd);
        row.bytes_verdict = bv.into();
        rows.push(row);
    }
    regressions.sort_by(|p: &Change, q: &Change| q.delta_pct.total_cmp(&p.delta_pct));
    improvements.sort_by(|p: &Change, q: &Change| p.delta_pct.total_cmp(&q.delta_pct));
    noisy.dedup();
    let mut skipped: Vec<SkippedRow> = vec![];
    for (side, r) in [("x", x), ("y", y)] {
        for s in &r.skipped {
            match skipped.iter_mut().find(|k| k.id == s.id) {
                Some(k) => k.side = "both".into(),
                None => skipped.push(SkippedRow { id: s.id.clone(), side: side.into(), reason: s.reason.clone() }),
            }
        }
    }
    let side = |r: &Report| Side { commit: r.commit.clone(), describe: r.describe.clone(), date: r.date.clone(), load: r.host.load, cores: r.host.cores, repeats: r.repeats };
    let verdict = if !regressions.is_empty() {
        "regression"
    } else if !improvements.is_empty() {
        "faster"
    } else {
        "ok"
    };
    Comparison {
        schema: 2,
        verdict: verdict.into(),
        exit_code: i32::from(!regressions.is_empty()),
        x: side(x),
        y: side(y),
        busy,
        threshold_time_pct: t.time_pct,
        threshold_bytes_pct: t.bytes_pct,
        threshold_allocs_pct: t.allocs_pct,
        rows,
        regressions,
        improvements,
        tradeoffs,
        noisy,
        changed,
        skipped,
        summary: sum,
    }
}

fn pct(d: Option<f64>) -> String {
    d.map(|d| format!("{d:+.1}%")).unwrap_or_else(|| "-".into())
}

/// The summary line: the last line of `compare.md` and the one line printed.
pub fn summary_line(c: &Comparison) -> String {
    let s = &c.summary;
    format!(
        "{}: {} scenarios compared ({} -> {}, {} and {} runs): {} regressions, {} slower, {} faster, {} noisy, {} changed, {} added, {} removed (thresholds: time {}%, allocs {}%, peak bytes {}%)",
        c.verdict,
        s.compared,
        c.x.describe,
        c.y.describe,
        c.x.repeats.max(1),
        c.y.repeats.max(1),
        s.regressions,
        s.slower,
        s.faster,
        s.noisy,
        s.changed,
        s.added,
        s.removed,
        c.threshold_time_pct,
        c.threshold_allocs_pct,
        c.threshold_bytes_pct
    )
}

/// A warning when a computer was busy during one of the runs: its times are larger and less steady.
pub fn busy_note(c: &Comparison) -> Option<String> {
    c.busy.then(|| {
        format!(
            "note: the computer was busy during a run (load average {} for X, {} for Y, {} and {} cores): times are less steady (a time regression under {LOW_CONFIDENCE_BELOW_PCT} % has confidence low: run it again with --filter), counts and peaks are not affected",
            c.x.load.map_or("unknown".to_string(), |l| format!("{l:.1}")),
            c.y.load.map_or("unknown".to_string(), |l| format!("{l:.1}")),
            c.x.cores,
            c.y.cores
        )
    })
}

fn change_table(out: &mut String, title: &str, list: &[Change], limit: usize) {
    if list.is_empty() {
        return;
    }
    let show = |m: &str, v: u64| match m {
        "time" => super::report::duration(v),
        "peak_bytes" => super::report::bytes(v),
        _ => v.to_string(),
    };
    out.push_str(&format!("## {title}\n\n| Scenario | Metric | X | Y | Δ | Confidence |\n|---|---|---:|---:|---:|---|\n"));
    for ch in list.iter().take(limit) {
        out.push_str(&format!("| `{}` | {} | {} | {} | {:+.1}% | {} |\n", ch.id, ch.metric, show(&ch.metric, ch.x), show(&ch.metric, ch.y), ch.delta_pct, ch.confidence));
    }
    if list.len() > limit {
        out.push_str(&format!("\n… and {} more (compare.json has all of them)\n", list.len() - limit));
    }
    out.push('\n');
}

pub fn markdown(c: &Comparison) -> String {
    let mut out = format!(
        "# Benchmark comparison: {}\n\nX (older): `{}` {} {} ({} runs)\n\nY (newer): `{}` {} {} ({} runs)\n\n",
        c.verdict,
        c.x.describe,
        c.x.commit.chars().take(12).collect::<String>(),
        c.x.date,
        c.x.repeats.max(1),
        c.y.describe,
        c.y.commit.chars().take(12).collect::<String>(),
        c.y.date,
        c.y.repeats.max(1)
    );
    if let Some(note) = busy_note(c) {
        out.push_str(&format!("{note}\n\n"));
    }
    change_table(&mut out, "Regressions", &c.regressions, 50);
    change_table(&mut out, "Improvements (the best first)", &c.improvements, 15);
    change_table(&mut out, "Trade-offs: more allocations or memory for a clearly faster scenario (not regressions)", &c.tradeoffs, 50);
    if !c.noisy.is_empty() {
        out.push_str(&format!("## Noisy: moved more than {}% inside the spread of the runs\n\nRun them again: `scripts/bench-compare.sh … --filter <id>`\n\n", c.threshold_time_pct));
        for id in &c.noisy {
            out.push_str(&format!("- `{id}`\n"));
        }
        out.push('\n');
    }
    if !c.changed.is_empty() {
        out.push_str("## Changed: no verdict, the scenario measures something else\n\n");
        for ch in &c.changed {
            out.push_str(&format!("- `{}` ({}, version {} -> {})\n", ch.id, ch.reason, ch.x_version.unwrap_or(1), ch.y_version.unwrap_or(1)));
        }
        out.push('\n');
    }
    if !c.skipped.is_empty() {
        out.push_str("## Skipped\n\n");
        for k in &c.skipped {
            out.push_str(&format!("- `{}` on {}: {}\n", k.id, k.side, k.reason));
        }
        out.push('\n');
    }
    out.push_str("## Every scenario\n\n| Scenario | X median | Y median | Δ time | Time | X allocs | Y allocs | Δ allocs | Allocs | X peak | Y peak | Δ peak | Peak |\n|---|---:|---:|---:|---|---:|---:|---:|---|---:|---:|---:|---|\n");
    for r in &c.rows {
        let d = |v: Option<u64>| v.map(super::report::duration).unwrap_or_else(|| "-".into());
        let b = |v: Option<u64>| v.map(super::report::bytes).unwrap_or_else(|| "-".into());
        let n = |v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        let (tv, av, bv) = if r.status == "added" || r.status == "removed" { (r.status.clone(), r.status.clone(), r.status.clone()) } else { (r.time_verdict.clone(), r.allocs_verdict.clone(), r.bytes_verdict.clone()) };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r.id,
            d(r.x_median_ns),
            d(r.y_median_ns),
            pct(r.time_delta_pct),
            tv,
            n(r.x_allocs),
            n(r.y_allocs),
            pct(r.allocs_delta_pct),
            av,
            b(r.x_peak_bytes),
            b(r.y_peak_bytes),
            pct(r.bytes_delta_pct),
            bv
        ));
    }
    out.push_str(&format!("\n{}\n", summary_line(c)));
    out
}

fn write_failure(out_dir: &Path, error: &str) {
    let f = Failure { schema: 2, verdict: "error".into(), exit_code: 2, error: error.into() };
    if std::fs::create_dir_all(out_dir).is_ok() {
        if let Ok(json) = serde_json::to_string_pretty(&f) {
            let _ = std::fs::write(out_dir.join("compare.json"), json + "\n");
        }
        let _ = std::fs::write(out_dir.join("compare.md"), format!("# Benchmark comparison: error\n\n{error}\n"));
    }
}

/// Read both files, write `compare.md` and `compare.json` into `out_dir`, print the summary line; the exit code.
pub fn run(x: &Path, y: &Path, t: Thresholds, forced_changed: &[String], out_dir: &Path) -> i32 {
    let go = || -> Result<Comparison, String> {
        let (rx, ry) = (read_report(x)?, read_report(y)?);
        if !rx.run.is_empty() && !ry.run.is_empty() && rx.run != ry.run {
            return Err(format!("{} ran the {} profile and {} the {} profile: their data differs, compare runs of one profile", x.display(), rx.run, y.display(), ry.run));
        }
        let c = compare(&rx, &ry, t, forced_changed);
        std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        std::fs::write(out_dir.join("compare.md"), markdown(&c)).map_err(|e| e.to_string())?;
        std::fs::write(out_dir.join("compare.json"), serde_json::to_string_pretty(&c).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
        Ok(c)
    };
    match go() {
        Ok(c) => {
            println!("{}", summary_line(&c));
            if let Some(note) = busy_note(&c) {
                println!("{note}");
            }
            for r in &c.regressions {
                println!("REGRESSION {} {} {:+.1}% ({} confidence)", r.id, r.metric, r.delta_pct, r.confidence);
            }
            for id in &c.noisy {
                println!("NOISY {id}: moved more than {}% inside the spread, run it again with --filter", t.time_pct);
            }
            c.exit_code
        }
        Err(e) => {
            eprintln!("shale: {e}");
            write_failure(out_dir, &e);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::merge::tests::{report, sc};
    use super::*;

    const T: Thresholds = Thresholds { time_pct: 10.0, bytes_pct: 15.0, allocs_pct: 10.0 };

    fn with_runs(mut s: ScenarioResult, medians_ms: &[u64]) -> ScenarioResult {
        s.run_medians_ns = medians_ms.iter().map(|m| m * 1_000_000).collect();
        s.median_ns = s.run_medians_ns.iter().copied().min().unwrap();
        s
    }

    // spec: BUILD-030
    #[test]
    fn verdicts_follow_the_thresholds_the_noise_band_and_the_floors() {
        let mut x = report("x", vec![sc("same", 100, 1000), sc("fast", 100, 1000), sc("slow", 100, 1000), sc("reg", 100, 1000), sc("mem", 100, 1000), sc("gone", 1, 1), sc("tiny", 0, 0)]);
        x.scenarios[6].median_ns = 1_000;
        let mut tiny = sc("tiny", 0, 0);
        tiny.median_ns = 5_000;
        let y = report("y", vec![sc("same", 103, 1000), sc("fast", 80, 1000), sc("slow", 108, 1000), sc("reg", 125, 1000), sc("mem", 100, 1300), sc("new", 1, 1), tiny]);
        let c = compare(&x, &y, T, &[]);
        let row = |id: &str| c.rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(row("same").time_verdict, "ok");
        assert_eq!(row("fast").time_verdict, "faster");
        assert_eq!(row("slow").time_verdict, "slower");
        assert_eq!(row("reg").time_verdict, "REGRESSION");
        assert_eq!(row("mem").bytes_verdict, "REGRESSION");
        assert_eq!(row("mem").time_verdict, "ok");
        assert_eq!(row("tiny").time_verdict, "ok", "+400% of 1 microsecond is below the floor");
        assert_eq!((row("gone").status.as_str(), row("new").status.as_str()), ("removed", "added"));
        let regs: Vec<_> = c.regressions.iter().map(|r| (r.id.as_str(), r.metric.as_str())).collect();
        assert_eq!(regs, [("mem", "peak_bytes"), ("reg", "time")], "the worst first");
        let better: Vec<_> = c.improvements.iter().map(|r| (r.id.as_str(), r.metric.as_str())).collect();
        assert_eq!(better, [("fast", "time")]);
        assert_eq!(c.summary, Summary { compared: 6, faster: 1, slower: 1, regressions: 2, noisy: 0, changed: 0, added: 1, removed: 1 });
        assert_eq!((c.verdict.as_str(), c.exit_code), ("regression", 1));
        assert_eq!(c.rows.last().unwrap().id, "gone", "what only X has is last");
    }

    // spec: BUILD-036
    #[test]
    fn a_time_difference_inside_the_spread_is_noisy_not_a_regression() {
        // X ran 100 to 140 ms between its runs, Y 120 to 150: the best medians differ by 20 %, the spreads overlap
        let x = report("x", vec![with_runs(sc("a", 100, 1000), &[100, 140, 120]), with_runs(sc("b", 100, 1000), &[100, 102, 101])]);
        let y = report("y", vec![with_runs(sc("a", 120, 1000), &[120, 150, 135]), with_runs(sc("b", 130, 1000), &[130, 133, 131])]);
        let c = compare(&x, &y, T, &[]);
        let row = |id: &str| c.rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(row("a").time_verdict, "noisy");
        assert_eq!(row("b").time_verdict, "REGRESSION", "the spreads of b are apart");
        assert_eq!(c.noisy, ["a"]);
        assert_eq!(c.regressions.len(), 1);
        assert_eq!(c.summary.noisy, 1);
        // a single run: the fastest to the slowest iteration is the spread
        let (mut sx, mut sy) = (sc("s", 100, 1), sc("s", 130, 1));
        sx.p95_ns = 135_000_000;
        sy.min_ns = 110_000_000;
        assert_eq!(compare(&report("x", vec![sx.clone()]), &report("y", vec![sy.clone()]), T, &[]).rows[0].time_verdict, "noisy");
        sx.p95_ns = 105_000_000;
        assert_eq!(compare(&report("x", vec![sx]), &report("y", vec![sy]), T, &[]).rows[0].time_verdict, "REGRESSION");
        // the same in the other direction is not an improvement
        let c = compare(&y, &x, T, &[]);
        assert_eq!(c.rows.iter().find(|r| r.id == "a").unwrap().time_verdict, "noisy");
        assert_eq!(c.improvements.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["b"]);
    }

    // spec: BUILD-036
    #[test]
    fn allocation_counts_and_peaks_are_exact_signals_whatever_the_load() {
        let mut busy_y = report("y", vec![sc("a", 100, 1000), sc("b", 100, 1000), sc("c", 100, 1000)]);
        busy_y.host.load = Some(90.0);
        busy_y.host.cores = 4;
        busy_y.scenarios[0].allocs = 12_000; // +20 %
        busy_y.scenarios[1].allocs = 9_000; // -10 %
        busy_y.scenarios[2].median_ns = 115_000_000; // +15 %, spreads apart, but a busy computer
        busy_y.scenarios[2].min_ns = 114_000_000;
        let x = report("x", vec![sc("a", 100, 1000), sc("b", 100, 1000), sc("c", 100, 1000)]);
        let c = compare(&x, &busy_y, T, &[]);
        assert!(c.busy);
        let reg = |id: &str, m: &str| c.regressions.iter().find(|r| r.id == id && r.metric == m);
        assert_eq!(reg("a", "allocs").unwrap().confidence, "high", "a count regression is clear on a busy computer");
        assert_eq!(c.rows[0].allocs_verdict, "REGRESSION");
        assert_eq!(c.rows[1].allocs_verdict, "fewer");
        assert_eq!(c.improvements.iter().map(|i| (i.id.as_str(), i.metric.as_str())).collect::<Vec<_>>(), [("b", "allocs")]);
        assert_eq!(reg("c", "time").unwrap().confidence, "low", "a time regression under 30 % on a busy computer is to be checked again");
        // a few allocations are noise whatever their share; 1 % of a count is ok
        let (mut p, mut q) = (sc("a", 1, 1), sc("a", 1, 1));
        (p.allocs, q.allocs) = (10, 400);
        assert_eq!(compare(&report("x", vec![p]), &report("y", vec![q]), T, &[]).rows[0].allocs_verdict, "ok");
        let (mut p, mut q) = (sc("a", 1, 1), sc("a", 1, 1));
        (p.allocs, q.allocs) = (100_000, 101_500);
        assert_eq!(compare(&report("x", vec![p]), &report("y", vec![q]), T, &[]).rows[0].allocs_verdict, "ok");
        assert!(busy_note(&c).unwrap().contains("busy"));
    }

    // spec: BUILD-036
    #[test]
    fn more_memory_for_a_clearly_faster_scenario_is_a_tradeoff_not_a_regression() {
        let x = report("x", vec![sc("tree", 100, 1000), sc("slow-and-big", 100, 1000)]);
        let mut y = report("y", vec![sc("tree", 40, 5000), sc("slow-and-big", 120, 5000)]);
        y.scenarios[0].allocs = 50_000;
        let c = compare(&x, &y, T, &[]);
        assert_eq!((c.rows[0].allocs_verdict.as_str(), c.rows[0].bytes_verdict.as_str()), ("tradeoff", "tradeoff"));
        assert_eq!(c.tradeoffs.iter().map(|t| t.metric.as_str()).collect::<Vec<_>>(), ["allocs", "peak_bytes"]);
        let regs: Vec<_> = c.regressions.iter().map(|r| (r.id.as_str(), r.metric.as_str())).collect();
        assert_eq!(regs, [("slow-and-big", "peak_bytes"), ("slow-and-big", "time")], "slower and bigger stays a regression");
        assert!(markdown(&c).contains("## Trade-offs"));
    }

    // spec: BUILD-036
    #[test]
    fn a_scenario_whose_version_changed_has_no_verdict() {
        let mut y = report("y", vec![sc("moved", 300, 5000), sc("same", 100, 1000), sc("old-file", 300, 5000)]);
        y.scenarios[0].version = 2;
        let x = report("x", vec![sc("moved", 100, 1000), sc("same", 100, 1000), sc("old-file", 100, 1000)]);
        let c = compare(&x, &y, T, &["old-file".into()]);
        let row = |id: &str| c.rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!((row("moved").status.as_str(), row("moved").time_verdict.as_str(), row("moved").allocs_verdict.as_str()), ("changed", "changed", "changed"));
        assert_eq!(row("old-file").status, "changed", "--changed names a scenario of results that carry no version");
        assert_eq!(c.changed.iter().map(|k| (k.id.as_str(), k.reason.as_str())).collect::<Vec<_>>(), [("moved", "version"), ("old-file", "--changed")]);
        assert!(c.regressions.is_empty() && c.verdict == "ok", "{:?}", c.regressions);
        assert_eq!(c.summary.changed, 2);
        let md = markdown(&c);
        assert!(md.contains("## Changed") && md.contains("`moved` (version, version 1 -> 2)"), "{md}");
        // a result of schema 1 has no version: it reads as version 1
        let old: ScenarioResult = serde_json::from_str(r#"{"id":"a","iters":1,"median_ns":1,"p95_ns":1,"min_ns":1,"allocs":1,"alloc_bytes":1,"peak_bytes":1}"#).unwrap();
        assert_eq!((old.version, old.run_medians_ns.len()), (1, 0));
    }

    // spec: BUILD-036
    #[test]
    fn the_overall_verdict_skipped_scenarios_and_the_markdown_lead_with_what_matters() {
        let mut x = report("x", vec![sc("a", 100, 1000), sc("b", 100, 1000)]);
        x.skipped.push(super::super::runner::Skipped { id: "git".into(), reason: "no git-lfs".into() });
        let mut y = report("y", vec![sc("a", 100, 1000), sc("b", 50, 1000)]);
        y.skipped.push(super::super::runner::Skipped { id: "git".into(), reason: "no git-lfs".into() });
        y.skipped.push(super::super::runner::Skipped { id: "tex".into(), reason: "no lualatex".into() });
        let c = compare(&x, &y, T, &[]);
        assert_eq!((c.verdict.as_str(), c.exit_code), ("faster", 0));
        assert_eq!(c.skipped.iter().map(|k| (k.id.as_str(), k.side.as_str())).collect::<Vec<_>>(), [("git", "both"), ("tex", "y")]);
        let md = markdown(&c);
        assert!(md.starts_with("# Benchmark comparison: faster"), "{md}");
        assert!(md.find("## Improvements").unwrap() < md.find("## Every scenario").unwrap(), "{md}");
        assert!(md.contains("## Skipped") && md.contains("no lualatex"));
        assert_eq!(compare(&x, &x, T, &[]).verdict, "ok");
        let j: serde_json::Value = serde_json::to_value(&c).unwrap();
        for k in ["schema", "verdict", "exit_code", "x", "y", "busy", "rows", "regressions", "improvements", "tradeoffs", "noisy", "changed", "skipped", "summary", "threshold_time_pct", "threshold_bytes_pct", "threshold_allocs_pct"] {
            assert!(j.get(k).is_some(), "{k}");
        }
    }

    // spec: BUILD-030, BUILD-031
    #[test]
    fn exit_codes_files_and_the_summary_line() {
        let d = std::env::temp_dir().join(format!("shale-bench-cmp-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let w = |n: &str, r: &Report| {
            std::fs::write(d.join(n), serde_json::to_string(r).unwrap()).unwrap();
            d.join(n)
        };
        let x = w("x.json", &report("x", vec![sc("a", 100, 1000)]));
        let ok = w("ok.json", &report("ok", vec![sc("a", 101, 1000)]));
        let bad = w("bad.json", &report("bad", vec![sc("a", 150, 1000)]));
        let mut busy = report("busy", vec![sc("a", 101, 1000)]);
        busy.host.load = Some(40.0);
        busy.host.cores = 4;
        let c = compare(&report("x", vec![sc("a", 100, 1000)]), &busy, T, &[]);
        assert!(markdown(&c).contains("the computer was busy"), "a busy run is said in compare.md");
        assert!(busy_note(&compare(&report("x", vec![]), &report("y", vec![]), T, &[])).is_none());
        assert_eq!(run(&x, &ok, T, &[], &d.join("o1")), 0);
        let md = std::fs::read_to_string(d.join("o1/compare.md")).unwrap();
        assert!(md.contains("| `a` |") && md.trim_end().ends_with(")"), "{md}");
        assert!(md.contains("0 regressions"));
        assert_eq!(run(&x, &bad, T, &[], &d.join("o2")), 1);
        let j: Comparison = serde_json::from_str(&std::fs::read_to_string(d.join("o2/compare.json")).unwrap()).unwrap();
        assert_eq!((j.regressions.len(), j.verdict.as_str(), j.exit_code), (1, "regression", 1));
        assert_eq!(run(&x, &bad, Thresholds { time_pct: 80.0, ..T }, &[], &d.join("o3")), 0, "a looser threshold lets it pass");
        assert_eq!(run(&x, &d.join("missing.json"), T, &[], &d.join("o4")), 2);
        let f: Failure = serde_json::from_str(&std::fs::read_to_string(d.join("o4/compare.json")).unwrap()).unwrap();
        assert_eq!((f.verdict.as_str(), f.exit_code), ("error", 2), "an error is in compare.json too, for an agent that reads only that");
        let mut quick = report("q", vec![sc("a", 100, 1000)]);
        quick.run = "quick".into();
        let quick = w("quick.json", &quick);
        assert_eq!(run(&x, &quick, T, &[], &d.join("o6")), 2, "runs of two profiles are not compared");
        std::fs::write(d.join("junk.json"), "x").unwrap();
        assert_eq!(run(&x, &d.join("junk.json"), T, &[], &d.join("o5")), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
