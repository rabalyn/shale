//! Measuring a scenario and the shape of a result file (BUILD-027, BUILD-028).

use super::alloc;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// The version of the result file's layout; a reader refuses another one.
pub const SCHEMA: u32 = 1;

/// One measured scenario.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ScenarioResult {
    pub id: String,
    pub iters: usize,
    pub median_ns: u64,
    pub p95_ns: u64,
    pub min_ns: u64,
    /// allocations in one iteration (the median of the iterations)
    pub allocs: u64,
    /// bytes asked for in one iteration (the median)
    pub alloc_bytes: u64,
    /// the most bytes alive above the start of an iteration (the largest of the iterations)
    pub peak_bytes: u64,
    /// bytes alive after the last iteration minus before the first: what the iterations left behind
    /// (a cache that only grows, a leak, an undo history); negative when the iterations freed memory
    #[serde(default)]
    pub retained_bytes: i64,
}

/// A scenario that did not run, and why.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Skipped {
    pub id: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Host {
    pub cpu: String,
    pub cores: usize,
    pub os: String,
    /// the one-minute load average at the end of the run (Linux): a busy computer makes every time larger
    #[serde(default)]
    pub load: Option<f64>,
}

/// A whole run: what `--bench <out.json>` writes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Report {
    pub schema: u32,
    pub commit: String,
    pub describe: String,
    /// UTC, `2026-11-18T10:00:00Z`
    pub date: String,
    pub host: Host,
    /// `release`, `bench-release` or `debug` (`SHALE_BENCH_PROFILE` names it, else the build says)
    pub profile: String,
    pub scenarios: Vec<ScenarioResult>,
    /// scenarios that could not run here (an external tool is missing): never a failure
    #[serde(default)]
    pub skipped: Vec<Skipped>,
    /// the run profile: `quick`, `full` or `worst`
    #[serde(default)]
    pub run: String,
    /// the process's peak resident memory (`VmHWM`), Linux only
    pub peak_rss_kb: Option<u64>,
}

/// The sample of one iteration.
struct Sample {
    ns: u64,
    allocs: u64,
    bytes: u64,
    peak: u64,
}

/// The loop that is measured, a function of its own so a profile can be cut down to it: the
/// scenario's setup, the warm-up and the vault generation stay out of a flamegraph that keeps
/// only the stacks through `measured_region` (`scripts/bench.sh --flamegraph`).
#[inline(never)]
fn measured_region(iters: usize, f: &mut dyn FnMut()) -> Vec<Sample> {
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let before = alloc::snapshot();
        alloc::reset_peak();
        let t = Instant::now();
        f();
        let ns = t.elapsed().as_nanos() as u64;
        let after = alloc::snapshot();
        samples.push(Sample { ns, allocs: after.allocs - before.allocs, bytes: after.bytes - before.bytes, peak: after.peak.saturating_sub(before.live) as u64 });
    }
    samples
}

/// The value at the middle (`pct` 50) or the nearest rank (`pct` 95) of the sorted values.
pub fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    if pct == 50 {
        let n = sorted.len();
        return if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2 };
    }
    let rank = (sorted.len() * pct).div_ceil(100).max(1);
    sorted[rank - 1]
}

fn sorted(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v
}

/// Run `f` once as a warm-up (unless it is too costly to run twice: `warmup` false), then `iters` times measured.
pub fn measure(id: &str, iters: usize, warmup: bool, min_ms: Option<u64>, f: &mut dyn FnMut()) -> ScenarioResult {
    if warmup {
        f();
    }
    let live_before = alloc::snapshot().live as i64;
    let mut samples = measured_region(iters, f);
    // `min_ms`: go on in rounds of `iters` until this much time was measured (a flamegraph needs samples)
    while min_ms.is_some_and(|ms| samples.iter().map(|s| s.ns).sum::<u64>() < ms * 1_000_000) {
        samples.extend(measured_region(iters, f));
    }
    let retained_bytes = alloc::snapshot().live as i64 - live_before;
    let times = sorted(samples.iter().map(|s| s.ns).collect());
    let allocs = sorted(samples.iter().map(|s| s.allocs).collect());
    let bytes = sorted(samples.iter().map(|s| s.bytes).collect());
    ScenarioResult {
        id: id.to_string(),
        iters: samples.len(),
        median_ns: percentile(&times, 50),
        p95_ns: percentile(&times, 95),
        min_ns: times.first().copied().unwrap_or(0),
        allocs: percentile(&allocs, 50),
        alloc_bytes: percentile(&bytes, 50),
        peak_bytes: samples.iter().map(|s| s.peak).max().unwrap_or(0),
        retained_bytes,
    }
}

/// The peak resident memory of this process in KiB (`VmHWM` of `/proc/self/status`); `None` off Linux.
pub fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.split_whitespace().next()?.parse().ok()
}

pub fn host() -> Host {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, v)| v.trim().to_string()))).unwrap_or_else(|| "unknown".into());
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|k| format!(" {}", k.trim())).unwrap_or_default();
    let load = std::fs::read_to_string("/proc/loadavg").ok().and_then(|t| t.split_whitespace().next()?.parse().ok());
    Host { load, cpu, cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), os: format!("{} {}{kernel}", std::env::consts::OS, std::env::consts::ARCH) }
}

/// `2026-11-18T10:00:00Z` for a count of seconds since 1970.
pub fn iso_date(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

fn git(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// The header of a result: commit, describe, date, host and profile. `SHALE_BENCH_COMMIT` and
/// `SHALE_BENCH_DESCRIBE` say them when the run is not inside the repository.
pub fn header(run: &str, scenarios: Vec<ScenarioResult>, skipped: Vec<Skipped>) -> Report {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    Report {
        schema: SCHEMA,
        commit: std::env::var("SHALE_BENCH_COMMIT").ok().or_else(|| git(&["rev-parse", "HEAD"])).unwrap_or_else(|| "unknown".into()),
        describe: std::env::var("SHALE_BENCH_DESCRIBE").ok().or_else(|| git(&["describe", "--tags", "--always", "--dirty"])).unwrap_or_else(|| "unknown".into()),
        date: iso_date(secs),
        host: host(),
        profile: std::env::var("SHALE_BENCH_PROFILE").unwrap_or_else(|_| if cfg!(debug_assertions) { "debug" } else { "release" }.into()),
        scenarios,
        skipped,
        run: run.into(),
        peak_rss_kb: peak_rss_kb(),
    }
}

/// Read a result file; a file of another schema is an error.
pub fn read_report(path: &std::path::Path) -> Result<Report, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let r: Report = serde_json::from_str(&text).map_err(|e| format!("{}: not a benchmark result ({e})", path.display()))?;
    if r.schema != SCHEMA {
        return Err(format!("{}: schema {} (this build reads schema {SCHEMA})", path.display(), r.schema));
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BUILD-028
    #[test]
    fn median_p95_and_min_are_read_from_the_sorted_times() {
        assert_eq!(percentile(&[], 50), 0);
        assert_eq!(percentile(&[5], 50), 5);
        assert_eq!(percentile(&[1, 3, 9], 50), 3);
        assert_eq!(percentile(&[1, 3, 5, 9], 50), 4, "the middle of two");
        let hundred: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&hundred, 95), 95, "nearest rank");
        assert_eq!(percentile(&[1, 2, 3], 95), 3);
    }

    // spec: BUILD-028
    #[test]
    fn a_scenario_is_run_once_to_warm_up_and_then_measured_with_its_allocations() {
        let mut runs = 0;
        let mut keep: Vec<Vec<u8>> = vec![];
        let r = measure("x", 4, true, None, &mut || {
            runs += 1;
            let v = vec![1u8; 100_000];
            std::hint::black_box(&v);
            if runs % 2 == 0 {
                keep.push(v);
            }
        });
        assert_eq!(runs, 5, "one warm-up and four measured runs");
        assert_eq!((r.id.as_str(), r.iters), ("x", 4));
        assert!(r.min_ns <= r.median_ns && r.median_ns <= r.p95_ns);
        assert!(r.allocs >= 1 && r.alloc_bytes >= 100_000, "{r:?}");
        assert!(r.peak_bytes >= 100_000);
        assert!(r.retained_bytes >= 150_000, "two of the four runs kept their buffer: {r:?}");
    }

    // spec: BUILD-028
    #[test]
    fn a_minimum_time_keeps_the_rounds_going() {
        let r = measure("slow", 2, false, Some(30), &mut || std::thread::sleep(std::time::Duration::from_millis(5)));
        assert!(r.iters >= 6 && r.iters.is_multiple_of(2), "{r:?}: rounds of two until 30 ms were measured");
    }

    // spec: BUILD-028
    #[test]
    fn the_date_is_iso_utc_and_the_report_round_trips_through_json() {
        assert_eq!(iso_date(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_date(crate::calendar::time::days_from_civil(2026, 11, 18) * 86_400 + 3661), "2026-11-18T01:01:01Z");
        let r = Report {
            schema: SCHEMA,
            commit: "abc".into(),
            describe: "v1".into(),
            date: iso_date(0),
            host: Host { cpu: "c".into(), cores: 2, os: "linux".into(), load: Some(0.5) },
            profile: "release".into(),
            skipped: vec![Skipped { id: "m".into(), reason: "r".into() }],
            run: "full".into(),
            scenarios: vec![ScenarioResult { id: "a.b".into(), iters: 3, median_ns: 5, p95_ns: 6, min_ns: 4, allocs: 7, alloc_bytes: 8, peak_bytes: 9, retained_bytes: -1 }],
            peak_rss_kb: Some(10),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), r);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        for k in ["schema", "commit", "describe", "date", "host", "profile", "scenarios", "peak_rss_kb"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        for k in ["id", "iters", "median_ns", "p95_ns", "min_ns", "allocs", "alloc_bytes", "peak_bytes"] {
            assert!(v["scenarios"][0].get(k).is_some(), "{k}");
        }
    }

    // spec: BUILD-028
    #[test]
    fn a_result_of_another_schema_is_refused() {
        let dir = std::env::temp_dir().join(format!("shale-bench-schema-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("r.json");
        std::fs::write(&f, r#"{"schema":2}"#).unwrap();
        assert!(read_report(&f).unwrap_err().contains("not a benchmark result"));
        let other = Report {
            schema: 2,
            commit: String::new(),
            describe: String::new(),
            date: String::new(),
            host: Host { cpu: String::new(), cores: 1, os: String::new(), load: None },
            profile: String::new(),
            scenarios: vec![],
            skipped: vec![],
            run: String::new(),
            peak_rss_kb: None,
        };
        std::fs::write(&f, serde_json::to_string(&other).unwrap()).unwrap();
        assert!(read_report(&f).unwrap_err().contains("schema 2"));
        std::fs::write(&f, "not json").unwrap();
        assert!(read_report(&f).is_err());
        assert!(read_report(&dir.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_peak_resident_memory_is_read_on_linux() {
        assert!(peak_rss_kb().unwrap() > 0);
    }
}
