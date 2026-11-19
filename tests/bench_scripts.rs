//! The refusals of the benchmark scripts (BUILD-030, BUILD-033, BUILD-034). Their real work is
//! building and running the benchmark binary, which `cargo test --features bench` and the
//! releases cover; here the arguments and the failure paths are checked. The release step itself
//! is tested in `scripts/test-release.sh` with a stand-in for the binary. Linux only: the scripts
//! are bash.
#![cfg(target_os = "linux")]

use std::path::Path;
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

// spec: BUILD-035
#[test]
fn build_035_land_sh_checks_the_benchmark_feature() {
    let land = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/land.sh")).unwrap();
    assert!(land.contains("cargo clippy -q --all-targets --locked --features bench -- -D warnings"));
    assert!(land.contains("cargo test -q --locked --features bench bench::"));
}
