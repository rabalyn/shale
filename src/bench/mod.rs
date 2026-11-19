//! The benchmark harness (BUILD-028…BUILD-038, raba/shale#343): only in a build with the cargo
//! feature `bench`, run as `shale --bench <out.json>`, headless. Scenarios measure the code
//! paths that decide how Shale feels (opening a vault, searching it, the link index, the graph,
//! frames of the window); every one reports time, allocations and peak memory, and
//! `--bench-compare` and `--bench-report` turn results into a verdict and a page. See
//! `docs/perf/README.md`.

mod alloc;
pub mod cli;
mod compare;
mod content_gen;
mod frames;
mod merge;
mod report;
mod runner;
mod scen_data;
mod scen_features;
mod scen_io;
mod scenarios;
mod vault_gen;
mod worst;

use cli::Command;
use std::path::Path;

/// Run a benchmark command; the exit code of the process.
pub fn run(command: Command) -> i32 {
    match command {
        Command::List => {
            for p in [scenarios::Profile::Quick, scenarios::Profile::Full, scenarios::Profile::Worst] {
                for id in scenarios::ids(p) {
                    println!("{} {id}", p.name());
                }
            }
            0
        }
        Command::Run { out, filter, iters, min_ms, profile } => finish(run_scenarios(&out, profile, filter.as_deref(), iters, min_ms)),
        Command::Compare { x, y, time_pct, bytes_pct, allocs_pct, changed, out_dir } => compare::run(&x, &y, compare::Thresholds { time_pct, bytes_pct, allocs_pct }, &changed, &out_dir),
        Command::Merge { out, runs } => merge::run(&out, &runs),
        Command::Report { dir, out } => finish(report::write(&dir, &out)),
    }
}

fn finish(r: Result<(), String>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("shale: {e}");
            1
        }
    }
}

fn run_scenarios(out: &Path, profile: scenarios::Profile, filter: Option<&str>, iters: Option<usize>, min_ms: Option<u64>) -> Result<(), String> {
    let report = scenarios::run(scenarios::Scale::of(profile), profile, filter, iters, min_ms, &mut |r| {
        eprintln!("{:<34} {:>12}  {:>9} allocs  {:>12} peak", r.id, report::duration(r.median_ns), r.allocs, report::bytes(r.peak_bytes));
    })?;
    let json = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(out, json + "\n").map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!("wrote {} ({} scenarios, {} skipped)", out.display(), report.scenarios.len(), report.skipped.len());
    Ok(())
}
