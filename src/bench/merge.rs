//! Merging repeated runs of one commit into one result (BUILD-036): `shale --bench-merge out.json
//! run1.json run2.json …`. A busy computer only ever makes a time larger, so the merged
//! `median_ns` is the smallest median of the runs, and `run_medians_ns` keeps all of them: their
//! range is the spread that `--bench-compare` weighs a time difference against. Counts and peaks
//! are exact, so they are the median of the runs (the runs of one binary agree).

use super::runner::{percentile, read_report, Report, ScenarioResult, Skipped};
use std::path::Path;

fn medians(s: &ScenarioResult) -> Vec<u64> {
    if s.run_medians_ns.is_empty() {
        vec![s.median_ns]
    } else {
        s.run_medians_ns.clone()
    }
}

fn middle(mut v: Vec<u64>) -> u64 {
    v.sort_unstable();
    percentile(&v, 50)
}

/// One result of several runs of the same commit and profile.
pub fn merge(runs: &[Report]) -> Result<Report, String> {
    let first = runs.first().ok_or("--bench-merge needs at least one run")?;
    for r in runs {
        if r.run != first.run {
            return Err(format!("the runs are of different profiles ({} and {})", first.run, r.run));
        }
        if r.commit != first.commit {
            return Err(format!("the runs are of different commits ({} and {})", first.commit, r.commit));
        }
    }
    let mut ids: Vec<&str> = vec![];
    for r in runs {
        for s in &r.scenarios {
            if !ids.contains(&s.id.as_str()) {
                ids.push(&s.id);
            }
        }
    }
    let scenarios = ids
        .iter()
        .map(|id| {
            let of: Vec<&ScenarioResult> = runs.iter().filter_map(|r| r.scenarios.iter().find(|s| s.id == *id)).collect();
            let all_medians: Vec<u64> = of.iter().flat_map(|s| medians(s)).collect();
            let best = of.iter().min_by_key(|s| s.median_ns).expect("at least one run has the scenario");
            let pick = |f: fn(&ScenarioResult) -> u64| middle(of.iter().map(|s| f(s)).collect());
            let mut retained: Vec<i64> = of.iter().map(|s| s.retained_bytes).collect();
            retained.sort_unstable();
            ScenarioResult {
                id: id.to_string(),
                iters: best.iters,
                median_ns: all_medians.iter().copied().min().unwrap_or(0),
                p95_ns: best.p95_ns,
                min_ns: of.iter().map(|s| s.min_ns).min().unwrap_or(0),
                allocs: pick(|s| s.allocs),
                alloc_bytes: pick(|s| s.alloc_bytes),
                peak_bytes: pick(|s| s.peak_bytes),
                retained_bytes: retained[retained.len() / 2],
                version: of.iter().map(|s| s.version).max().unwrap_or(1),
                run_medians_ns: if all_medians.len() > 1 { all_medians } else { vec![] },
            }
        })
        .collect();
    let mut skipped: Vec<Skipped> = vec![];
    for s in runs.iter().flat_map(|r| &r.skipped) {
        if !skipped.iter().any(|k| k.id == s.id) {
            skipped.push(s.clone());
        }
    }
    let last = runs.last().expect("not empty");
    let mut host = last.host.clone();
    host.load = runs.iter().filter_map(|r| r.host.load).fold(None, |m: Option<f64>, l| Some(m.map_or(l, |m| m.max(l))));
    Ok(Report {
        schema: super::runner::SCHEMA,
        commit: first.commit.clone(),
        describe: first.describe.clone(),
        date: last.date.clone(),
        host,
        profile: first.profile.clone(),
        scenarios,
        skipped,
        run: first.run.clone(),
        peak_rss_kb: runs.iter().filter_map(|r| r.peak_rss_kb).max(),
        repeats: runs.iter().map(|r| r.repeats.max(1)).sum(),
    })
}

/// Read the runs, write the merged result; the exit code.
pub fn run(out: &Path, runs: &[std::path::PathBuf]) -> i32 {
    let go = || -> Result<Report, String> {
        let reports: Result<Vec<Report>, String> = runs.iter().map(|p| read_report(p)).collect();
        let merged = merge(&reports?)?;
        let json = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(out, json + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(merged)
    };
    match go() {
        Ok(m) => {
            println!("merged {} runs of {} into {} ({} scenarios)", m.repeats, m.describe, out.display(), m.scenarios.len());
            0
        }
        Err(e) => {
            eprintln!("shale: {e}");
            2
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::runner::Host;
    use super::*;

    pub fn sc(id: &str, ms: u64, peak_kb: u64) -> ScenarioResult {
        ScenarioResult { id: id.into(), iters: 5, median_ns: ms * 1_000_000, p95_ns: ms * 1_020_000, min_ns: ms * 980_000, allocs: 10_000, alloc_bytes: 100, peak_bytes: peak_kb * 1024, retained_bytes: 0, version: 1, run_medians_ns: vec![] }
    }

    pub fn report(name: &str, s: Vec<ScenarioResult>) -> Report {
        Report {
            schema: super::super::runner::SCHEMA,
            commit: format!("{name}0000"),
            describe: name.into(),
            date: "2026-01-01T00:00:00Z".into(),
            host: Host { cpu: "c".into(), cores: 1, os: "o".into(), load: None },
            profile: "release".into(),
            scenarios: s,
            peak_rss_kb: None,
            skipped: vec![],
            run: "full".into(),
            repeats: 1,
        }
    }

    // spec: BUILD-036
    #[test]
    fn runs_of_one_commit_merge_into_the_best_median_with_the_spread_kept() {
        let mut a = report("c", vec![sc("t", 120, 1000), sc("only-a", 5, 1)]);
        let mut b = report("c", vec![sc("t", 100, 1000)]);
        let mut c = report("c", vec![sc("t", 110, 1004)]);
        a.host.load = Some(30.0);
        b.host.load = Some(2.0);
        c.skipped.push(Skipped { id: "s".into(), reason: "no tool".into() });
        c.scenarios[0].allocs = 10_040;
        c.date = "2026-01-02T00:00:00Z".into();
        let m = merge(&[a, b, c]).unwrap();
        let t = m.scenarios.iter().find(|s| s.id == "t").unwrap();
        assert_eq!(t.median_ns, 100_000_000, "the smallest median: a busy computer only adds time");
        assert_eq!(t.run_medians_ns, [120_000_000, 100_000_000, 110_000_000], "all medians stay in the order of the runs");
        assert_eq!(t.allocs, 10_000, "the middle count of the runs");
        assert_eq!(t.peak_bytes, 1000 * 1024);
        assert_eq!((m.repeats, m.host.load, m.date.as_str()), (3, Some(30.0), "2026-01-02T00:00:00Z"), "the worst load, the last date");
        assert_eq!(m.skipped.len(), 1);
        let only = m.scenarios.iter().find(|s| s.id == "only-a").unwrap();
        assert!(only.run_medians_ns.is_empty(), "one run of a scenario has no spread of runs");
        // merged files merge again: the runs add up
        let again = merge(&[m.clone(), report("c", vec![sc("t", 90, 1000)])]).unwrap();
        assert_eq!((again.repeats, again.scenarios[0].run_medians_ns.len(), again.scenarios[0].median_ns), (4, 4, 90_000_000));
    }

    // spec: BUILD-036
    #[test]
    fn runs_of_other_commits_or_profiles_do_not_merge() {
        assert!(merge(&[]).is_err());
        assert!(merge(&[report("a", vec![]), report("b", vec![])]).unwrap_err().contains("different commits"));
        let mut q = report("a", vec![]);
        q.run = "quick".into();
        assert!(merge(&[report("a", vec![]), q]).unwrap_err().contains("different profiles"));
    }

    // spec: BUILD-036
    #[test]
    fn the_merge_command_reads_and_writes_files_and_ends_with_an_exit_code() {
        let d = std::env::temp_dir().join(format!("shale-bench-merge-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let w = |n: &str, r: &Report| {
            std::fs::write(d.join(n), serde_json::to_string(r).unwrap()).unwrap();
            d.join(n)
        };
        let runs = [w("a.json", &report("c", vec![sc("t", 10, 1)])), w("b.json", &report("c", vec![sc("t", 12, 1)]))];
        assert_eq!(run(&d.join("sub/m.json"), &runs), 0);
        let m = read_report(&d.join("sub/m.json")).unwrap();
        assert_eq!((m.repeats, m.scenarios[0].median_ns), (2, 10_000_000));
        assert_eq!(run(&d.join("m2.json"), &[d.join("missing.json")]), 2);
        assert_eq!(run(&d.join("m3.json"), &[]), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
