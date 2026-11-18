//! Comparing two result files (BUILD-030): `shale --bench-compare x.json y.json` says, per
//! scenario, whether Y (the newer run) is faster, slower or a regression against X, writes
//! `compare.md` and `compare.json` and ends with exit code 0 (no regression), 1 (a regression)
//! or 2 (an error), so a release script or an agent can act on it.

use super::runner::{read_report, Report, ScenarioResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct Thresholds {
    /// a median time this many percent above X is a regression
    pub time_pct: f64,
    /// a peak above X by this many percent is a regression
    pub bytes_pct: f64,
}

/// Differences within this many percent are noise: `ok`.
pub const NOISE_PCT: f64 = 5.0;
/// A time difference below this is noise whatever its percentage (a scenario of a few microseconds).
pub const TIME_FLOOR_NS: u64 = 20_000;
/// A peak difference below this is noise whatever its percentage.
pub const BYTES_FLOOR: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Side {
    pub commit: String,
    pub describe: String,
    pub date: String,
}

/// One scenario of the comparison. `status` is `both`, `added` (only in Y) or `removed` (only in X).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub status: String,
    pub x_median_ns: Option<u64>,
    pub y_median_ns: Option<u64>,
    pub time_delta_pct: Option<f64>,
    /// `ok`, `faster`, `slower` or `REGRESSION` (`-` for added and removed)
    pub time_verdict: String,
    pub x_peak_bytes: Option<u64>,
    pub y_peak_bytes: Option<u64>,
    pub bytes_delta_pct: Option<f64>,
    pub bytes_verdict: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Regression {
    pub id: String,
    /// `time` or `peak_bytes`
    pub metric: String,
    pub x: u64,
    pub y: u64,
    pub delta_pct: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub compared: usize,
    pub faster: usize,
    pub slower: usize,
    pub regressions: usize,
    pub added: usize,
    pub removed: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Comparison {
    pub schema: u32,
    pub x: Side,
    pub y: Side,
    pub threshold_time_pct: f64,
    pub threshold_bytes_pct: f64,
    pub rows: Vec<Row>,
    pub regressions: Vec<Regression>,
    pub summary: Summary,
}

fn delta(x: u64, y: u64) -> f64 {
    if x == 0 {
        return if y == 0 { 0.0 } else { 100.0 };
    }
    (y as f64 - x as f64) / x as f64 * 100.0
}

/// The verdict of a change of `x` to `y`: a regression needs both the percentage and the absolute floor.
fn verdict(x: u64, y: u64, limit_pct: f64, floor: u64) -> (f64, &'static str) {
    let d = delta(x, y);
    let big = x.abs_diff(y) >= floor;
    let v = if d > limit_pct && big {
        "REGRESSION"
    } else if d > NOISE_PCT && big {
        "slower"
    } else if d < -NOISE_PCT && big {
        "faster"
    } else {
        "ok"
    };
    (d, v)
}

pub fn compare(x: &Report, y: &Report, t: Thresholds) -> Comparison {
    let xs: BTreeMap<&str, &ScenarioResult> = x.scenarios.iter().map(|s| (s.id.as_str(), s)).collect();
    let ys: BTreeMap<&str, &ScenarioResult> = y.scenarios.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut ids: Vec<&str> = xs.keys().chain(ys.keys()).copied().collect();
    // the order of Y (the newer run), then what only X has
    ids.sort_by_key(|id| (y.scenarios.iter().position(|s| s.id == *id).unwrap_or(usize::MAX), *id));
    ids.dedup();
    let (mut rows, mut regressions) = (vec![], vec![]);
    let mut sum = Summary { compared: 0, faster: 0, slower: 0, regressions: 0, added: 0, removed: 0 };
    for id in ids {
        match (xs.get(id), ys.get(id)) {
            (Some(a), Some(b)) => {
                let (td, tv) = verdict(a.median_ns, b.median_ns, t.time_pct, TIME_FLOOR_NS);
                let (bd, bv) = verdict(a.peak_bytes, b.peak_bytes, t.bytes_pct, BYTES_FLOOR);
                sum.compared += 1;
                for (v, metric, xv, yv, d) in [(tv, "time", a.median_ns, b.median_ns, td), (bv, "peak_bytes", a.peak_bytes, b.peak_bytes, bd)] {
                    if v == "REGRESSION" {
                        regressions.push(Regression { id: id.to_string(), metric: metric.into(), x: xv, y: yv, delta_pct: d });
                    }
                }
                if tv == "REGRESSION" || bv == "REGRESSION" {
                    sum.regressions += 1;
                } else if tv == "slower" || bv == "slower" {
                    sum.slower += 1;
                } else if tv == "faster" || bv == "faster" {
                    sum.faster += 1;
                }
                rows.push(Row {
                    id: id.into(),
                    status: "both".into(),
                    x_median_ns: Some(a.median_ns),
                    y_median_ns: Some(b.median_ns),
                    time_delta_pct: Some(td),
                    time_verdict: tv.into(),
                    x_peak_bytes: Some(a.peak_bytes),
                    y_peak_bytes: Some(b.peak_bytes),
                    bytes_delta_pct: Some(bd),
                    bytes_verdict: bv.into(),
                });
            }
            (a, b) => {
                let status = if a.is_some() { "removed" } else { "added" };
                if a.is_some() {
                    sum.removed += 1;
                } else {
                    sum.added += 1;
                }
                rows.push(Row {
                    id: id.into(),
                    status: status.into(),
                    x_median_ns: a.map(|s| s.median_ns),
                    y_median_ns: b.map(|s| s.median_ns),
                    time_delta_pct: None,
                    time_verdict: "-".into(),
                    x_peak_bytes: a.map(|s| s.peak_bytes),
                    y_peak_bytes: b.map(|s| s.peak_bytes),
                    bytes_delta_pct: None,
                    bytes_verdict: "-".into(),
                });
            }
        }
    }
    let side = |r: &Report| Side { commit: r.commit.clone(), describe: r.describe.clone(), date: r.date.clone() };
    Comparison { schema: 1, x: side(x), y: side(y), threshold_time_pct: t.time_pct, threshold_bytes_pct: t.bytes_pct, rows, regressions, summary: sum }
}

fn pct(d: Option<f64>) -> String {
    d.map(|d| format!("{d:+.1}%")).unwrap_or_else(|| "-".into())
}

/// The summary line: the last line of `compare.md` and the one line printed.
pub fn summary_line(c: &Comparison) -> String {
    let s = &c.summary;
    format!(
        "{} scenarios compared ({} -> {}): {} regressions, {} slower, {} faster, {} added, {} removed (thresholds: time {}%, peak bytes {}%)",
        s.compared, c.x.describe, c.y.describe, s.regressions, s.slower, s.faster, s.added, s.removed, c.threshold_time_pct, c.threshold_bytes_pct
    )
}

pub fn markdown(c: &Comparison) -> String {
    let mut out = format!(
        "# Benchmark comparison\n\nX (older): `{}` {} {}\n\nY (newer): `{}` {} {}\n\n",
        c.x.describe,
        c.x.commit.chars().take(12).collect::<String>(),
        c.x.date,
        c.y.describe,
        c.y.commit.chars().take(12).collect::<String>(),
        c.y.date
    );
    out.push_str("| Scenario | X median | Y median | Δ time | Time | X peak | Y peak | Δ peak | Peak |\n|---|---:|---:|---:|---|---:|---:|---:|---|\n");
    for r in &c.rows {
        let d = |v: Option<u64>| v.map(super::report::duration).unwrap_or_else(|| "-".into());
        let b = |v: Option<u64>| v.map(super::report::bytes).unwrap_or_else(|| "-".into());
        let (tv, bv) = if r.status == "both" { (r.time_verdict.clone(), r.bytes_verdict.clone()) } else { (r.status.clone(), r.status.clone()) };
        out.push_str(&format!("| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |\n", r.id, d(r.x_median_ns), d(r.y_median_ns), pct(r.time_delta_pct), tv, b(r.x_peak_bytes), b(r.y_peak_bytes), pct(r.bytes_delta_pct), bv));
    }
    out.push_str(&format!("\n{}\n", summary_line(c)));
    out
}

/// Read both files, write `compare.md` and `compare.json` into `out_dir`, print the summary line; the exit code.
pub fn run(x: &Path, y: &Path, t: Thresholds, out_dir: &Path) -> i32 {
    let go = || -> Result<Comparison, String> {
        let (rx, ry) = (read_report(x)?, read_report(y)?);
        if !rx.run.is_empty() && !ry.run.is_empty() && rx.run != ry.run {
            return Err(format!("{} ran the {} profile and {} the {} profile: their data differs, compare runs of one profile", x.display(), rx.run, y.display(), ry.run));
        }
        let c = compare(&rx, &ry, t);
        std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        std::fs::write(out_dir.join("compare.md"), markdown(&c)).map_err(|e| e.to_string())?;
        std::fs::write(out_dir.join("compare.json"), serde_json::to_string_pretty(&c).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
        Ok(c)
    };
    match go() {
        Ok(c) => {
            println!("{}", summary_line(&c));
            for r in &c.regressions {
                println!("REGRESSION {} {} {:+.1}%", r.id, r.metric, r.delta_pct);
            }
            i32::from(!c.regressions.is_empty())
        }
        Err(e) => {
            eprintln!("shale: {e}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::runner::{Host, SCHEMA};
    use super::*;

    fn sc(id: &str, ms: u64, peak_kb: u64) -> ScenarioResult {
        ScenarioResult { id: id.into(), iters: 5, median_ns: ms * 1_000_000, p95_ns: ms * 1_100_000, min_ns: ms * 900_000, allocs: 10, alloc_bytes: 100, peak_bytes: peak_kb * 1024, retained_bytes: 0 }
    }

    pub fn report(name: &str, s: Vec<ScenarioResult>) -> Report {
        Report {
            schema: SCHEMA,
            commit: format!("{name}0000"),
            describe: name.into(),
            date: "2026-01-01T00:00:00Z".into(),
            host: Host { cpu: "c".into(), cores: 1, os: "o".into() },
            profile: "release".into(),
            scenarios: s,
            peak_rss_kb: None,
            skipped: vec![],
            run: "full".into(),
        }
    }

    const T: Thresholds = Thresholds { time_pct: 10.0, bytes_pct: 15.0 };

    // spec: BUILD-030
    #[test]
    fn verdicts_follow_the_thresholds_the_noise_band_and_the_floors() {
        let x = report("x", vec![sc("same", 100, 1000), sc("fast", 100, 1000), sc("slow", 100, 1000), sc("reg", 100, 1000), sc("mem", 100, 1000), sc("gone", 1, 1), sc("tiny", 0, 0)]);
        let mut tiny = sc("tiny", 0, 0);
        tiny.median_ns = 5_000;
        let mut tiny_x = sc("tiny", 0, 0);
        tiny_x.median_ns = 1_000;
        let mut x = x;
        x.scenarios[6] = tiny_x;
        let y = report("y", vec![sc("same", 103, 1000), sc("fast", 80, 1000), sc("slow", 108, 1000), sc("reg", 125, 1000), sc("mem", 100, 1300), sc("new", 1, 1), tiny]);
        let c = compare(&x, &y, T);
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
        assert_eq!(regs, [("reg", "time"), ("mem", "peak_bytes")]);
        assert_eq!(c.summary, Summary { compared: 6, faster: 1, slower: 1, regressions: 2, added: 1, removed: 1 });
        assert_eq!(c.rows.last().unwrap().id, "gone", "what only X has is last");
    }

    // spec: BUILD-030
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
        assert_eq!(run(&x, &ok, T, &d.join("o1")), 0);
        let md = std::fs::read_to_string(d.join("o1/compare.md")).unwrap();
        assert!(md.contains("| `a` |") && md.trim_end().ends_with(")"), "{md}");
        assert!(md.contains("0 regressions"));
        assert_eq!(run(&x, &bad, T, &d.join("o2")), 1);
        let j: Comparison = serde_json::from_str(&std::fs::read_to_string(d.join("o2/compare.json")).unwrap()).unwrap();
        assert_eq!(j.regressions.len(), 1);
        assert_eq!(run(&x, &bad, Thresholds { time_pct: 80.0, bytes_pct: 15.0 }, &d.join("o3")), 0, "a looser threshold lets it pass");
        assert_eq!(run(&x, &d.join("missing.json"), T, &d.join("o4")), 2);
        let mut quick = report("q", vec![sc("a", 100, 1000)]);
        quick.run = "quick".into();
        let quick = w("quick.json", &quick);
        assert_eq!(run(&x, &quick, T, &d.join("o6")), 2, "runs of two profiles are not compared");
        std::fs::write(d.join("junk.json"), "x").unwrap();
        assert_eq!(run(&x, &d.join("junk.json"), T, &d.join("o5")), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
