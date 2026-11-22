//! The static page of the results (BUILD-032): `shale --bench-report docs/perf docs/perf/index.html`
//! reads every `results/*.json` and draws, without any script or outside file, a table of the
//! latest run and, per scenario, the median time and the peak memory over the runs, with a link to
//! the flamegraphs of the release (an attachment of its GitHub release, not a folder in git, #400).

use super::runner::{read_report, Report};
use std::path::Path;

/// `1.5 ms`, `830 µs`, `2.31 s`
pub fn duration(ns: u64) -> String {
    match ns {
        0..=999 => format!("{ns} ns"),
        1_000..=999_999 => format!("{:.1} µs", ns as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.2} ms", ns as f64 / 1e6),
        _ => format!("{:.2} s", ns as f64 / 1e9),
    }
}

/// `512 B`, `3.4 MiB`
pub fn bytes(b: u64) -> String {
    match b {
        0..=1023 => format!("{b} B"),
        1024..=1_048_575 => format!("{:.1} KiB", b as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} MiB", b as f64 / 1_048_576.0),
        _ => format!("{:.2} GiB", b as f64 / 1_073_741_824.0),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// One run with the name of its file (`v0.6.0`, `baseline-5c2b794`).
pub struct Run {
    pub label: String,
    pub report: Report,
}

/// The runs of a folder (`results/`, `results-worst/`), oldest first (by the date in the file).
pub fn read_runs(results: &Path) -> Result<Vec<Run>, String> {
    let mut runs = vec![];
    let entries = std::fs::read_dir(results).map_err(|e| format!("{}: {e}", results.display()))?;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") {
            let label = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            runs.push(Run { label, report: read_report(&p)? });
        }
    }
    runs.sort_by(|a, b| a.report.date.cmp(&b.report.date).then(a.label.cmp(&b.label)));
    Ok(runs)
}

/// A line chart of `points` (label, value) as inline SVG; `fmt` writes a value.
fn chart(points: &[(String, u64)], fmt: fn(u64) -> String) -> String {
    let (w, h, pad) = (300.0, 90.0, 8.0);
    let max = points.iter().map(|p| p.1).max().unwrap_or(1).max(1) as f64;
    let xy = |i: usize, v: u64| {
        let x = if points.len() < 2 { w / 2.0 } else { pad + (w - 2.0 * pad) * i as f64 / (points.len() - 1) as f64 };
        (x, h - pad - (h - 2.0 * pad) * v as f64 / max)
    };
    let mut s = format!("<svg viewBox=\"0 0 {w} {h}\" role=\"img\"><line x1=\"{pad}\" y1=\"{y}\" x2=\"{x2}\" y2=\"{y}\" class=\"axis\"/>", y = h - pad, x2 = w - pad);
    let line: Vec<String> = points.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", xy(i, p.1).0, xy(i, p.1).1)).collect();
    if points.len() > 1 {
        s.push_str(&format!("<polyline points=\"{}\" class=\"line\"/>", line.join(" ")));
    }
    for (i, (label, v)) in points.iter().enumerate() {
        let (x, y) = xy(i, *v);
        s.push_str(&format!("<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"3.5\" class=\"dot\"><title>{}: {}</title></circle>", esc(label), esc(&fmt(*v))));
    }
    s.push_str(&format!("<text x=\"{pad}\" y=\"12\" class=\"tick\">max {}</text></svg>", esc(&fmt(max as u64))));
    s
}

const STYLE: &str = "<style>\n\
:root{--bg:#fff;--fg:#1d1d1f;--mute:#666;--line:#d8d8de;--acc:#2f6fdb;--bad:#c62828;--good:#2e7d32}\n\
@media (prefers-color-scheme:dark){:root{--bg:#16161a;--fg:#e8e8ec;--mute:#9a9aa5;--line:#34343c;--acc:#7aa7ff;--bad:#ff6b6b;--good:#6bd17a}}\n\
body{font:14px/1.45 system-ui,sans-serif;background:var(--bg);color:var(--fg);margin:0 auto;padding:16px;max-width:1200px}\n\
h1{font-size:22px}h2{font-size:16px;margin-top:28px}table{border-collapse:collapse;width:100%}th,td{text-align:right;padding:4px 8px;border-bottom:1px solid var(--line)}\n\
th:first-child,td:first-child{text-align:left}code{font-size:13px}.mute{color:var(--mute)}.bad{color:var(--bad)}.good{color:var(--good)}\n\
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:12px}.card{border:1px solid var(--line);border-radius:6px;padding:8px}\n\
.card h3{font-size:13px;margin:0 0 4px}svg{width:100%;height:auto}.axis{stroke:var(--line)}.line{fill:none;stroke:var(--acc);stroke-width:2}.dot{fill:var(--acc)}.tick{font-size:10px;fill:var(--mute)}\n\
a{color:var(--acc)}\n</style>";

const RELEASES: &str = "https://github.com/rabalyn/shale/releases";

/// `0.7.1` of the label `v0.7.1`; `None` for a label that is not a release (`baseline-4ec690f`, a pre-release `v0.8.0-rc.1`).
fn release_version(label: &str) -> Option<&str> {
    let v = label.strip_prefix('v')?;
    let parts: Vec<&str> = v.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))).then_some(v)
}

/// Where the flamegraphs are (BUILD-041, #400): the release build on GitHub packs them into
/// `shale-flamegraphs-<version>.tar.gz` (a folder `flamegraphs/` of `<scenario id>.svg`) and attaches it to the
/// release page, so the latest run is linked to its release's attachment when it is a release; any other run
/// (a baseline, a branch) points to the releases page.
fn flamegraph_note(label: &str) -> String {
    match release_version(label) {
        Some(v) => format!(
            "<p class=\"mute\">Flamegraphs of {0}: <a href=\"{RELEASES}/download/v{1}/shale-flamegraphs-{1}.tar.gz\">shale-flamegraphs-{1}.tar.gz</a>, an attachment of the <a href=\"{RELEASES}/tag/v{1}\">release</a> (unpack it: one <code>&lt;scenario&gt;.svg</code> for each scenario that was flagged or among the slowest). Older releases: <a href=\"{RELEASES}\">releases</a>.</p>\n",
            esc(label),
            esc(v)
        ),
        None => format!("<p class=\"mute\">Flamegraphs are not kept in git: each release has them as the attachment <code>shale-flamegraphs-&lt;version&gt;.tar.gz</code> on its <a href=\"{RELEASES}\">release page</a>.</p>\n"),
    }
}

/// One set of runs (the regular runs, the worst case): the latest run as a table, the history as charts.
fn section(out: &mut String, title: &str, runs: &[Run]) {
    let Some(latest) = runs.last() else {
        out.push_str(&format!("<h2>{}</h2>\n<p>No results yet: run <code>scripts/bench.sh</code> and put the file in <code>docs/perf/</code>.</p>\n", esc(title)));
        return;
    };
    let r = &latest.report;
    out.push_str(&format!(
        "<h2>{}</h2>\n<p class=\"mute\">Latest run: <b>{}</b> ({} / <code>{}</code>), {}, {}, {} cores, {} profile, run <i>{}</i>, peak RSS {}. {} runs in all: {}.</p>\n",
        esc(title),
        esc(&latest.label),
        esc(&r.describe),
        esc(&r.commit.chars().take(10).collect::<String>()),
        esc(&r.date),
        esc(&r.host.cpu),
        r.host.cores,
        esc(&r.profile),
        esc(&r.run),
        r.peak_rss_kb.map(|k| bytes(k * 1024)).unwrap_or_else(|| "-".into()),
        runs.len(),
        esc(&runs.iter().map(|r| r.label.as_str()).collect::<Vec<_>>().join(", "))
    ));
    let prev = runs.len().checked_sub(2).map(|i| &runs[i]);
    out.push_str("<table><tr><th>Scenario</th><th>Median</th><th>p95</th><th>Min</th><th>Allocs</th><th>Allocated</th><th>Peak</th><th>Kept</th>");
    if let Some(p) = prev {
        out.push_str(&format!("<th>Median vs {}</th>", esc(&p.label)));
    }
    out.push_str("</tr>\n");
    for s in &r.scenarios {
        let change = prev.and_then(|p| p.report.scenarios.iter().find(|o| o.id == s.id)).map(|o| {
            let d = (s.median_ns as f64 - o.median_ns as f64) / o.median_ns.max(1) as f64 * 100.0;
            let class = if d > super::compare::NOISE_PCT {
                "bad"
            } else if d < -super::compare::NOISE_PCT {
                "good"
            } else {
                "mute"
            };
            format!("<span class=\"{class}\">{d:+.1}%</span>")
        });
        out.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>{}</tr>\n",
            esc(&s.id),
            duration(s.median_ns),
            duration(s.p95_ns),
            duration(s.min_ns),
            s.allocs,
            bytes(s.alloc_bytes),
            bytes(s.peak_bytes),
            if s.retained_bytes < 0 { format!("-{}", bytes(s.retained_bytes.unsigned_abs())) } else { bytes(s.retained_bytes as u64) },
            if prev.is_some() { format!("<td>{}</td>", change.unwrap_or_else(|| "-".into())) } else { String::new() }
        ));
    }
    out.push_str("</table>\n");
    out.push_str(&flamegraph_note(&latest.label));
    if !r.skipped.is_empty() {
        out.push_str("<p class=\"mute\">Skipped in the latest run:</p>\n<ul>");
        for k in &r.skipped {
            out.push_str(&format!("<li><code>{}</code>: {}</li>", esc(&k.id), esc(&k.reason)));
        }
        out.push_str("</ul>\n");
    }
    out.push_str("<p class=\"mute\">Median time and peak memory per scenario over the runs, oldest on the left.</p>\n<div class=\"grid\">\n");
    for s in &r.scenarios {
        let series = |f: fn(&super::runner::ScenarioResult) -> u64| -> Vec<(String, u64)> { runs.iter().filter_map(|run| run.report.scenarios.iter().find(|o| o.id == s.id).map(|o| (run.label.clone(), f(o)))).collect() };
        out.push_str(&format!("<div class=\"card\"><h3><code>{}</code></h3>{}{}</div>\n", esc(&s.id), chart(&series(|o| o.median_ns), duration), chart(&series(|o| o.peak_bytes), bytes)));
    }
    out.push_str("</div>\n");
}

/// The page: one section for each set of runs, `(title, runs)`.
pub fn page(sets: &[(&str, Vec<Run>)]) -> String {
    let mut out =
        format!("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Shale performance</title>\n{STYLE}</head><body>\n<h1>Shale performance</h1>\n");
    for (title, runs) in sets {
        section(&mut out, title, runs);
    }
    out.push_str("</body></html>\n");
    out
}

pub fn write(dir: &Path, out: &Path) -> Result<(), String> {
    let regular = read_runs(&dir.join("results"))?;
    // the worst case has runs of its own, when a release made some
    let worst = if dir.join("results-worst").is_dir() { read_runs(&dir.join("results-worst"))? } else { vec![] };
    let count = regular.len() + worst.len();
    let sets = [("Every scenario (the full profile)", regular), ("The worst case (the worst profile)", worst)];
    let sets: Vec<(&str, Vec<Run>)> = sets.into_iter().filter(|(_, r)| !r.is_empty()).collect();
    let sets = if sets.is_empty() { vec![("Every scenario (the full profile)", vec![])] } else { sets };
    std::fs::write(out, page(&sets)).map_err(|e| format!("{}: {e}", out.display()))?;
    eprintln!("wrote {} from {count} runs", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::runner::{Host, ScenarioResult, SCHEMA};
    use super::*;

    fn run(label: &str, date: &str, median_ms: u64) -> Run {
        let sc = ScenarioResult { id: "ui.frame.<x>".into(), iters: 3, median_ns: median_ms * 1_000_000, p95_ns: 0, min_ns: 0, allocs: 5, alloc_bytes: 6, peak_bytes: 2048, retained_bytes: 0, version: 1, run_medians_ns: vec![] };
        Run {
            label: label.into(),
            report: Report {
                schema: SCHEMA,
                commit: "abcdef0123456789".into(),
                describe: label.into(),
                date: date.into(),
                host: Host { cpu: "cpu".into(), cores: 4, os: "linux".into(), load: None },
                profile: "release".into(),
                scenarios: vec![sc],
                skipped: vec![],
                run: "full".into(),
                peak_rss_kb: Some(2048),
                repeats: 1,
            },
        }
    }

    // spec: BUILD-032
    #[test]
    fn durations_and_sizes_are_written_for_people() {
        assert_eq!(duration(500), "500 ns");
        assert_eq!(duration(1_500), "1.5 µs");
        assert_eq!(duration(2_500_000), "2.50 ms");
        assert_eq!(duration(3_000_000_000), "3.00 s");
        assert_eq!(bytes(100), "100 B");
        assert_eq!(bytes(2048), "2.0 KiB");
        assert_eq!(bytes(5 * 1_048_576), "5.0 MiB");
    }

    // spec: BUILD-032
    #[test]
    fn the_page_is_self_contained_and_shows_the_table_the_charts_and_the_flamegraph_link() {
        let html = page(&[("All", vec![run("v0.1.0", "2026-01-01", 10), run("v0.2.0", "2026-02-01", 20)])]);
        assert!(html.starts_with("<!doctype html>"));
        assert!(!html.contains("<script") && !html.contains("src=") && !html.contains("<link"), "no script and no file loaded from outside");
        assert!(html.contains("ui.frame.&lt;x&gt;"), "IDs are escaped");
        assert!(html.contains("<polyline"), "two runs draw a line");
        assert!(html.contains("+100.0%") && html.contains("class=\"bad\""), "the change against the run before");
        assert!(!html.contains("flamegraphs/"), "no link into a folder of git");
        assert!(html.contains("2 runs in all"));
        assert!(page(&[("All", vec![])]).contains("No results yet"));
    }

    // spec: BUILD-032, BUILD-041
    #[test]
    fn the_flamegraphs_are_linked_to_the_attachment_of_the_latest_release() {
        // the latest run is a release: its own attachment and release page
        let html = page(&[("All", vec![run("v0.1.0", "2026-01-01", 10), run("v0.2.0", "2026-02-01", 20)])]);
        assert!(html.contains("href=\"https://github.com/rabalyn/shale/releases/download/v0.2.0/shale-flamegraphs-0.2.0.tar.gz\""), "{html}");
        assert!(html.contains("href=\"https://github.com/rabalyn/shale/releases/tag/v0.2.0\""));
        assert!(!html.contains("download/v0.1.0"), "only the latest run");
        // not a release (a baseline, a pre-release): the releases page, no file named
        for label in ["baseline-4ec690f", "v0.3.0-rc.1", "v1.2", "vx.y.z", "0.2.0"] {
            let html = page(&[("All", vec![run(label, "2026-01-01", 10)])]);
            assert!(!html.contains("/download/") && html.contains("href=\"https://github.com/rabalyn/shale/releases\""), "{label}: {html}");
        }
        assert_eq!((release_version("v10.0.12"), release_version("v0.7.0-rc.2"), release_version("v0..1")), (Some("10.0.12"), None, None));
    }

    // spec: BUILD-032
    #[test]
    fn runs_are_read_from_the_results_folder_oldest_first() {
        let d = std::env::temp_dir().join(format!("shale-bench-report-{}", std::process::id()));
        std::fs::create_dir_all(d.join("results")).unwrap();
        for (l, date) in [("v2", "2026-02-01"), ("baseline-x", "2026-01-01")] {
            std::fs::write(d.join(format!("results/{l}.json")), serde_json::to_string(&run(l, date, 1).report).unwrap()).unwrap();
        }
        std::fs::write(d.join("results/notes.txt"), "ignored").unwrap();
        let runs = read_runs(&d.join("results")).unwrap();
        assert_eq!(runs.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["baseline-x", "v2"]);
        write(&d, &d.join("index.html")).unwrap();
        assert!(std::fs::read_to_string(d.join("index.html")).unwrap().contains("baseline-x"));
        assert!(read_runs(&d.join("nope")).is_err());
        std::fs::create_dir_all(d.join("results-worst")).unwrap();
        std::fs::write(d.join("results-worst/v9.json"), serde_json::to_string(&run("v9", "2026-03-01", 3).report).unwrap()).unwrap();
        write(&d, &d.join("index.html")).unwrap();
        let html = std::fs::read_to_string(d.join("index.html")).unwrap();
        assert!(html.contains("The worst case") && html.contains("v9"), "the worst runs have a section");
        let _ = std::fs::remove_dir_all(&d);
    }
}
