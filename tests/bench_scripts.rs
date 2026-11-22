//! The refusals of the benchmark scripts (BUILD-030, BUILD-033, BUILD-034, BUILD-041). Their real work is
//! building and running the benchmark binary, which `cargo test --features bench` and the
//! releases cover; here the arguments and the failure paths are checked. The release step itself
//! is tested in `scripts/test-release.sh` with a stand-in for the binary. Linux only: the scripts
//! are bash.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn script(name: &str, args: &[&str]) -> (i32, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("bash").arg(root.join("scripts").join(name)).args(args).current_dir(root).env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").output().expect("bash runs");
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

// spec: BUILD-033
#[test]
fn build_033_bench_sh_checks_its_arguments_before_it_builds() {
    let (code, text) = script("bench.sh", &["--help"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("--flamegraph") && text.contains("--profile"), "{text}");
    let (code, text) = script("bench.sh", &["--profile", "slow"]);
    assert_eq!((code, text.contains("quick, full or worst")), (2, true), "{text}");
    let (code, text) = script("bench.sh", &["--nonsense"]);
    assert_eq!((code, text.contains("unknown option --nonsense")), (2, true), "{text}");
}

// spec: BUILD-030
#[test]
fn build_030_bench_compare_sh_needs_two_commits_that_have_the_harness() {
    let (code, text) = script("bench-compare.sh", &[]);
    assert_eq!((code, text.contains("usage:")), (2, true), "{text}");
    let (code, text) = script("bench-compare.sh", &["HEAD", "HEAD", "--profile", "slow"]);
    assert_eq!(code, 2, "{text}");
    let (code, text) = script("bench-compare.sh", &["not-a-commit-xyz", "HEAD"]);
    assert_eq!((code, text.contains("is not a commit")), (2, true), "{text}");
    // the first commit of the repository is older than the harness. A shallow clone has no such commit: its oldest
    // commit is the shallow boundary (HEAD in CI, which checks out at depth 1), not the real first commit, so the
    // refusal cannot be tried there (HEAD against HEAD would run the full benchmarks)
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).current_dir(root).output().expect("git runs");
        String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_string()
    };
    if git(&["rev-parse", "--is-shallow-repository"]) == "true" {
        return;
    }
    let first = git(&["rev-list", "--max-parents=0", "HEAD"]);
    if first.is_empty() || first == git(&["rev-parse", "HEAD"]) {
        return;
    }
    let (code, text) = script("bench-compare.sh", &[&first, "HEAD", "--out", &std::env::temp_dir().join(format!("shale-bench-cmp-test-{}", std::process::id())).to_string_lossy()]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("has no benchmark harness"), "a commit without the feature is refused clearly: {text}");
}

// spec: BUILD-037
#[test]
fn build_037_bench_compare_sh_checks_the_options_of_the_agent_flow_before_it_builds() {
    let (code, text) = script("bench-compare.sh", &["--help"]);
    assert_eq!(code, 0, "{text}");
    for word in ["--runs", "--filter", "--changed", "--max-load", "--flamegraphs", "compare.json", "interleaved"] {
        assert!(text.contains(word), "{word}: {text}");
    }
    let (code, text) = script("bench-compare.sh", &["HEAD", "HEAD", "--runs", "0"]);
    assert_eq!((code, text.contains("--runs needs a number")), (2, true), "{text}");
    let (code, text) = script("bench-compare.sh", &["HEAD", "HEAD", "--runs", "many"]);
    assert_eq!((code, text.contains("--runs needs a number")), (2, true), "{text}");
    let (code, text) = script("bench-compare.sh", &["HEAD", "HEAD", "--filter"]);
    assert_eq!((code, text.contains("--filter needs a value")), (2, true), "{text}");
    let (code, text) = script("bench-compare.sh", &["HEAD", "HEAD", "--nonsense"]);
    assert_eq!((code, text.contains("unknown option --nonsense")), (2, true), "{text}");
    // the flamegraph script can be pointed at the build of another commit and keeps the stacks for the diff
    let (code, text) = script("bench.sh", &["--help"]);
    assert!(code == 0 && text.contains("--bin") && text.contains("--keep-folded"), "{text}");
}

// spec: BUILD-037
#[test]
fn build_037_a_run_that_cannot_start_leaves_a_compare_json_that_says_error() {
    let out = std::env::temp_dir().join(format!("shale-bench-err-{}", std::process::id()));
    let (code, text) = script("bench-compare.sh", &["not-a-commit-xyz", "HEAD", "--out", &out.to_string_lossy()]);
    assert_eq!(code, 2, "{text}");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(out.join("compare.json")).expect("compare.json is written on an error")).unwrap();
    assert_eq!((json["verdict"].as_str(), json["exit_code"].as_i64()), (Some("error"), Some(2)));
    assert!(json["error"].as_str().unwrap().contains("is not a commit"), "{json}");
    let _ = std::fs::remove_dir_all(&out);
}

// spec: BUILD-034
#[test]
fn build_034_bench_release_sh_checks_its_arguments() {
    let (code, text) = script("bench-release.sh", &[]);
    assert_eq!((code, text.contains("usage:")), (2, true), "{text}");
    let (code, text) = script("bench-release.sh", &["/no/such/folder", "1.2.3"]);
    assert_eq!((code, text.contains("is not a folder")), (2, true), "{text}");
    let (code, _) = script("bench-release.sh", &["--help"]);
    assert_eq!(code, 0);
}

// spec: FORGE-011
#[test]
fn forge_011_land_sh_runs_the_tests_with_nextest_and_needs_it() {
    let land = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/land.sh")).unwrap();
    // the missing tool stops the landing before the first merge, with the install command (#451)
    let need = land.find("cargo install cargo-nextest --locked").unwrap();
    assert!(need < land.find("git worktree add").unwrap());
    // the main test step is nextest, with the thread budget passed on, and there is no libtest step for it
    assert!(land.contains("step \"cargo nextest run\" cargo nextest run --locked --no-fail-fast"));
    assert!(land.contains("--test-threads \"$threads\"") && land.contains("RUST_TEST_THREADS"));
    assert!(!land.contains("step \"cargo test\""));
}

// spec: BUILD-035
#[test]
fn build_035_land_sh_checks_the_benchmark_feature() {
    let land = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/land.sh")).unwrap();
    assert!(land.contains("cargo clippy -q --all-targets --locked --features bench -- -D warnings"));
    // clippy on every landing, the tests only by path (#449) or with --release
    let clippy = land.find("cargo clippy -q --all-targets --locked --features bench -- -D warnings").unwrap();
    let gate = land.find("sh scripts/bench-tests-wanted.sh").unwrap();
    let tests = land.find("cargo test -q --locked --features bench bench::").unwrap();
    assert!(clippy < gate && gate < tests);
    assert!(land[..clippy].lines().last().unwrap().trim_start().starts_with("step "), "clippy is gated");
    assert!(land[gate - 40..gate].contains("\"$RELEASE\" = yes"));
    assert!(land.contains("-- SKIPPED (the change touches neither the benchmark harness nor the build)"));
}

/// A folder with a stand-in for the benchmark program and for `gh`: `--bench` writes a result,
/// `--bench-compare` writes `compare.json` from the file `compare-json` of the folder (exit code from `compare-exit`),
/// `gh release list` names the tags in `releases`, and `gh release download <tag>` has an asset only for the tags named
/// in `assets` (a line `tag system`).
fn bench_fakes(name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("shale-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("fake-shale");
    std::fs::write(
        &bin,
        r##"#!/bin/sh
echo "$*" >> "$(dirname "$0")/calls.log"
case "$1" in
  --bench) printf '{"schema":2,"describe":"%s","run":"%s"}\n' "$SHALE_BENCH_DESCRIBE" "$4" > "$2" ;;
  --bench-compare)
    while [ $# -gt 0 ]; do [ "$1" = --out ] && o=$2; shift; done
    mkdir -p "$o"
    echo "# compare" > "$o/compare.md"
    cp "$(dirname "$0")/compare-json" "$o/compare.json"
    exit "$(cat "$(dirname "$0")/compare-exit")" ;;
esac
"##,
    )
    .unwrap();
    let gh = dir.join("fake-gh");
    std::fs::write(
        &gh,
        r##"#!/bin/sh
d=$(dirname "$0")
echo "gh $*" >> "$d/calls.log"
case "$1 $2" in
  "release list") cat "$d/releases" ;;
  "release download")
    tag=$3
    while [ $# -gt 0 ]; do [ "$1" = --dir ] && o=$2; [ "$1" = --pattern ] && p=$2; shift; done
    sys=${p#shale-bench-*-}
    grep -qx "$tag ${sys%.json}" "$d/assets" || { echo "no assets match the file pattern" >&2; exit 1; }
    mkdir -p "$o"
    echo '{"schema":2,"describe":"'"$tag"'"}' > "$o/shale-bench-${tag#v}-${sys}" ;;
esac
"##,
    )
    .unwrap();
    for f in [&bin, &gh] {
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

fn github_run(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let summary = dir.join("summary.md");
    let _ = std::fs::remove_file(&summary);
    let out = Command::new("bash")
        .arg(root.join("scripts/bench-github.sh"))
        .args(args)
        .arg("--out")
        .arg(dir.join("out"))
        .current_dir(root)
        .env("BENCH_BIN", dir.join("fake-shale"))
        .env("BENCH_GH", dir.join("fake-gh"))
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_OUTPUT", dir.join("output"))
        .env("GITHUB_SHA", "0123456789abcdef")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("bash runs");
    (out.status.code().unwrap_or(-1), std::fs::read_to_string(&summary).unwrap_or_default(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

// spec: BUILD-041
#[test]
fn build_041_bench_github_sh_checks_its_arguments() {
    let (code, text) = script("bench-github.sh", &[]);
    assert_eq!((code, text.contains("usage:")), (2, true), "{text}");
    let (code, text) = script("bench-github.sh", &["v1.2.3", "amiga"]);
    assert_eq!((code, text.contains("the system is linux-x86_64")), (2, true), "{text}");
    let (code, text) = script("bench-github.sh", &["--help"]);
    assert!(code == 0 && text.contains("continue-on-error") && text.contains("--out"), "{text}");
    let (code, text) = script("bench-github.sh", &["v1.2.3", "linux-x86_64", "--nonsense"]);
    assert_eq!((code, text.contains("unknown option --nonsense")), (2, true), "{text}");
}

// spec: BUILD-041
#[test]
fn build_041_a_release_run_is_compared_with_the_previous_result_of_the_same_system_and_never_fails() {
    let dir = bench_fakes("bench-github");
    std::fs::write(
        dir.join("compare-json"),
        r#"{"verdict":"regression","x":{"describe":"v0.7.0","load":1.5},"y":{"load":2.5},
 "summary":{"compared":80,"faster":3,"slower":2,"regressions":2,"noisy":1,"changed":0},
 "regressions":[{"id":"ui.frame.x","metric":"time","x":1000000,"y":2000000,"delta_pct":100.0,"confidence":"low"},
                {"id":"vault.open.large","metric":"allocs","x":1000,"y":1500,"delta_pct":50.0,"confidence":"high"}],
 "noisy":["index.tasks.large"],
 "improvements":[{"id":"ui.frame.idle.large","metric":"time","x":30000000,"y":1000000,"delta_pct":-96.7,"confidence":"high"}]}"#,
    )
    .unwrap();
    std::fs::write(dir.join("compare-exit"), "1").unwrap();
    // v0.7.1 is the release itself, v0.7.0 has no benchmark asset yet, v0.6.9 has a linux one but no macos one
    std::fs::write(dir.join("releases"), "v0.7.1\nv0.7.0\nv0.6.9\n").unwrap();
    std::fs::write(dir.join("assets"), "v0.6.9 linux-x86_64\n").unwrap();

    let (code, summary, text) = github_run(&dir, &["v0.7.1", "linux-x86_64"]);
    // a regression is a finding in the summary, not a failure
    assert_eq!(code, 0, "{text}");
    let calls = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert!(calls.contains("--profile full") && !calls.contains("--profile worst") && !calls.contains("--profile quick"), "{calls}");
    assert!(dir.join("out/shale-bench-0.7.1-linux-x86_64.json").is_file());
    assert!(std::fs::read_to_string(dir.join("out/shale-bench-0.7.1-linux-x86_64.json")).unwrap().contains("\"describe\":\"v0.7.1\""));
    // the previous result is the first earlier release with an asset of this system (not the one without)
    assert!(calls.contains("gh release download v0.7.0") && calls.contains("gh release download v0.6.9"), "{calls}");
    assert!(!calls.contains("gh release download v0.7.1"), "the release is not its own predecessor: {calls}");
    assert!(calls.contains("--bench-compare") && calls.contains("shale-bench-0.6.9-linux-x86_64.json") && calls.contains("shale-bench-0.7.1-linux-x86_64.json"), "{calls}");
    for want in [
        "## Benchmarks: linux-x86_64, v0.7.1 (full profile)",
        "Statistics only",
        "Compared with `v0.6.9` on the same system: **regression**",
        "| `ui.frame.x` | time | 1 ms | 2 ms | 100 % | low |",
        "| `vault.open.large` | allocs | 1000 | 1500 | 50 % | high |",
        "`index.tasks.large`",
        "| `ui.frame.idle.large` | time | 30 ms | 1 ms | -96.7 % | high |",
    ] {
        assert!(summary.contains(want), "{want}: {summary}");
    }
    let output = std::fs::read_to_string(dir.join("output")).unwrap();
    assert!(output.contains("result=") && output.contains("compared=v0.6.9"), "{output}");

    // another system finds no earlier asset: nothing is compared, and that is said
    let _ = std::fs::remove_file(dir.join("calls.log"));
    let (code, summary, text) = github_run(&dir, &["v0.7.1", "macos-arm64"]);
    assert_eq!(code, 0, "{text}");
    assert!(summary.contains("No earlier release has a result for macos-arm64"), "{summary}");
    assert!(!std::fs::read_to_string(dir.join("calls.log")).unwrap().contains("--bench-compare"));

    // a comparison that cannot be made (another profile, a schema) is a line in the summary, not a failure
    std::fs::write(dir.join("compare-json"), r#"{"schema":2,"verdict":"error","exit_code":2,"error":"two profiles"}"#).unwrap();
    std::fs::write(dir.join("compare-exit"), "2").unwrap();
    let (code, summary, text) = github_run(&dir, &["v0.7.1", "linux-x86_64"]);
    assert_eq!(code, 0, "{text}");
    assert!(summary.contains("The comparison with v0.6.9 failed (exit code 2): two profiles"), "{summary}");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: BUILD-041
#[test]
fn build_041_a_run_that_does_not_run_is_reported_in_the_summary() {
    let dir = bench_fakes("bench-github-fail");
    std::fs::write(dir.join("fake-shale"), "#!/bin/sh\nexit 3\n").unwrap();
    let (code, summary, _) = github_run(&dir, &["v0.7.1", "windows-x86_64"]);
    assert_eq!(code, 1);
    assert!(summary.contains("the run failed") && summary.contains("No result for v0.7.1"), "{summary}");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: BUILD-034
#[test]
fn build_034_bench_release_sh_confirms_time_regressions_and_names_its_note_file() {
    let (code, text) = script("bench-release.sh", &["--help"]);
    assert_eq!(code, 0, "{text}");
    for word in ["--note", "BENCH_RECHECK_RUNS", "interleaved", "exact numbers"] {
        assert!(text.contains(word), "{word}: {text}");
    }
    let (code, text) = script("bench-release.sh", &["/tmp", "1.2.3", "--note"]);
    assert_eq!((code, text.contains("--note needs a file")), (2, true), "{text}");
}
