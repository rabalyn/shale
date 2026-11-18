//! The command line of the benchmark harness (BUILD-027): four modes of their own, chosen by the
//! first argument, so they never mix with the options of the window (BUILD-019 style: an unknown
//! option or a missing value is an error, exit code 2).

use super::scenarios::Profile;
use std::path::PathBuf;

pub const USAGE: &str = "usage: shale --bench <out.json> [--profile quick|full|worst] [--filter <substring>] [--iters N] [--min-ms MS]\n       shale --bench-list\n       shale --bench-compare <x.json> <y.json> [--threshold PCT] [--bytes-threshold PCT] [--out DIR]\n       shale --bench-report <perf dir> <out.html>";

/// What the benchmark options asked for.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// `--bench <out.json> [--profile p] [--filter s] [--iters N]`: run the scenarios of the profile (default `full`) whose ID contains `filter` (`=id`: the ID is `id`)
    Run { out: PathBuf, filter: Option<String>, iters: Option<usize>, min_ms: Option<u64>, profile: Profile },
    /// `--bench-list`: print the profile and ID of every scenario
    List,
    /// `--bench-compare x.json y.json`: X is the older run, Y the newer one
    Compare { x: PathBuf, y: PathBuf, time_pct: f64, bytes_pct: f64, out_dir: PathBuf },
    /// `--bench-report <perf dir> <out.html>`: the static page of `docs/perf`
    Report { dir: PathBuf, out: PathBuf },
}

/// A regression of the median time above this many percent fails a comparison (BUILD-030).
pub const DEFAULT_TIME_PCT: f64 = 10.0;
/// A regression of the peak bytes above this many percent fails a comparison (BUILD-030).
pub const DEFAULT_BYTES_PCT: f64 = 15.0;

/// `None` when the arguments are not a benchmark mode (the window's own parser reads them), else
/// the command or the message for stderr.
pub fn parse(args: &[String]) -> Option<Result<Command, String>> {
    let first = args.first()?.as_str();
    if !matches!(first, "--bench" | "--bench-list" | "--bench-compare" | "--bench-report") {
        return None;
    }
    Some(read(first, &args[1..]))
}

fn pct(opt: &str, v: &str) -> Result<f64, String> {
    v.parse::<f64>().ok().filter(|p| p.is_finite() && *p >= 0.0).ok_or_else(|| format!("{opt} needs a percentage like 10, not {v}"))
}

fn read(mode: &str, rest: &[String]) -> Result<Command, String> {
    let mut it = rest.iter();
    let mut positional: Vec<&str> = vec![];
    let (mut filter, mut iters, mut out_dir, mut profile, mut min_ms) = (None, None, None, Profile::Full, None);
    let (mut time_pct, mut bytes_pct) = (DEFAULT_TIME_PCT, DEFAULT_BYTES_PCT);
    while let Some(a) = it.next() {
        let mut value = || it.next().map(String::as_str).ok_or_else(|| format!("{a} needs a value"));
        match (mode, a.as_str()) {
            ("--bench", "--filter") => filter = Some(value()?.to_string()),
            ("--bench", "--profile") => {
                let v = value()?;
                profile = Profile::parse(v).ok_or_else(|| format!("--profile needs quick, full or worst, not {v}"))?;
            }
            ("--bench", "--min-ms") => {
                let v = value()?;
                min_ms = Some(v.parse::<u64>().map_err(|_| format!("--min-ms needs a number of milliseconds, not {v}"))?);
            }
            ("--bench", "--iters") => {
                let v = value()?;
                iters = Some(v.parse::<usize>().ok().filter(|n| *n >= 1).ok_or_else(|| format!("--iters needs a number of at least 1, not {v}"))?);
            }
            ("--bench-compare", "--threshold") => time_pct = pct("--threshold", value()?)?,
            ("--bench-compare", "--bytes-threshold") => bytes_pct = pct("--bytes-threshold", value()?)?,
            ("--bench-compare", "--out") => out_dir = Some(PathBuf::from(value()?)),
            (_, o) if o.starts_with('-') => return Err(format!("unknown option {o} for {mode}")),
            (_, word) => positional.push(word),
        }
    }
    let need = |n: usize, what: &str| if positional.len() == n { Ok(()) } else { Err(format!("{mode} needs {what}")) };
    match mode {
        "--bench" => {
            need(1, "one output file (out.json)")?;
            Ok(Command::Run { out: positional[0].into(), filter, iters, min_ms, profile })
        }
        "--bench-list" => {
            need(0, "no other arguments")?;
            Ok(Command::List)
        }
        "--bench-compare" => {
            need(2, "two result files (x.json y.json)")?;
            Ok(Command::Compare { x: positional[0].into(), y: positional[1].into(), time_pct, bytes_pct, out_dir: out_dir.unwrap_or_else(|| PathBuf::from(".")) })
        }
        _ => {
            need(2, "the perf folder and the output file (docs/perf out.html)")?;
            Ok(Command::Report { dir: positional[0].into(), out: positional[1].into() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Option<Result<Command, String>> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    // spec: BUILD-027
    #[test]
    fn only_the_bench_options_are_taken_and_the_rest_is_left_to_the_window() {
        assert!(p(&[]).is_none());
        assert!(p(&["--vault", "/v"]).is_none());
        assert!(p(&["--screenshot", "a.png"]).is_none());
        assert!(p(&["/notes"]).is_none());
    }

    // spec: BUILD-027
    #[test]
    fn bench_takes_an_output_file_a_filter_and_a_count() {
        assert_eq!(p(&["--bench", "o.json"]), Some(Ok(Command::Run { out: "o.json".into(), filter: None, iters: None, min_ms: None, profile: Profile::Full })));
        assert_eq!(
            p(&["--bench", "o.json", "--filter", "ui.", "--iters", "7", "--min-ms", "500", "--profile", "worst"]),
            Some(Ok(Command::Run { out: "o.json".into(), filter: Some("ui.".into()), iters: Some(7), min_ms: Some(500), profile: Profile::Worst }))
        );
        assert_eq!(p(&["--bench", "--iters", "3", "o.json"]), Some(Ok(Command::Run { out: "o.json".into(), filter: None, iters: Some(3), min_ms: None, profile: Profile::Full })), "the order does not matter");
    }

    // spec: BUILD-027
    #[test]
    fn a_bad_bench_command_line_is_an_error_not_a_guess() {
        let bad = |a: &[&str]| p(a).unwrap().unwrap_err();
        assert_eq!(bad(&["--bench"]), "--bench needs one output file (out.json)");
        assert_eq!(bad(&["--bench", "a", "b"]), "--bench needs one output file (out.json)");
        assert_eq!(bad(&["--bench", "o", "--iters"]), "--iters needs a value");
        assert!(bad(&["--bench", "o", "--iters", "0"]).contains("at least 1"));
        assert!(bad(&["--bench", "o", "--iters", "x"]).contains("not x"));
        assert_eq!(bad(&["--bench", "o", "--filtre", "a"]), "unknown option --filtre for --bench");
        assert_eq!(bad(&["--bench", "o", "--threshold", "5"]), "unknown option --threshold for --bench", "an option of another mode");
        assert_eq!(bad(&["--bench", "o", "--profile", "slow"]), "--profile needs quick, full or worst, not slow");
        assert_eq!(bad(&["--bench", "o", "--min-ms", "x"]), "--min-ms needs a number of milliseconds, not x");
        assert_eq!(bad(&["--bench-list", "x"]), "--bench-list needs no other arguments");
    }

    // spec: BUILD-030
    #[test]
    fn compare_takes_two_files_thresholds_and_an_output_folder() {
        assert_eq!(p(&["--bench-compare", "x.json", "y.json"]), Some(Ok(Command::Compare { x: "x.json".into(), y: "y.json".into(), time_pct: 10.0, bytes_pct: 15.0, out_dir: ".".into() })));
        assert_eq!(
            p(&["--bench-compare", "x.json", "y.json", "--threshold", "5", "--bytes-threshold", "20.5", "--out", "d"]),
            Some(Ok(Command::Compare { x: "x.json".into(), y: "y.json".into(), time_pct: 5.0, bytes_pct: 20.5, out_dir: "d".into() }))
        );
        let bad = |a: &[&str]| p(a).unwrap().unwrap_err();
        assert_eq!(bad(&["--bench-compare", "x.json"]), "--bench-compare needs two result files (x.json y.json)");
        assert!(bad(&["--bench-compare", "x", "y", "--threshold", "-1"]).contains("percentage"));
        assert!(bad(&["--bench-compare", "x", "y", "--threshold", "abc"]).contains("not abc"));
    }

    // spec: BUILD-032
    #[test]
    fn report_takes_the_perf_folder_and_the_page() {
        assert_eq!(p(&["--bench-report", "docs/perf", "i.html"]), Some(Ok(Command::Report { dir: "docs/perf".into(), out: "i.html".into() })));
        assert!(p(&["--bench-report", "docs/perf"]).unwrap().is_err());
    }
}
