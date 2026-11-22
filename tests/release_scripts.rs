//! The shell tests of the landing and tag scripts run with `cargo test`, so `scripts/land.sh`
//! checks them before every landing (FORGE-011, FORGE-013, FORGE-014, FORGE-016, ADR-0018): Forgejo's CI runs
//! only for tags now. Linux only: the scripts use GNU `sed -i` and `flock`.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

fn run(script: &str) {
    run_with(script, None);
}

/// Runs `script`; `prev_published` is what `SHALE_PREV_PUBLISHED` is set to (not set when `None`):
/// a release that runs `cargo test` must not make its last published release reach these scripts (#406).
fn run_with(script: &str, prev_published: Option<&str>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut cmd = Command::new("bash");
    cmd.arg(root.join(script)).current_dir(root).env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("LAND_CHECK_CMD").env_remove("SHALE_PREV_PUBLISHED");
    if let Some(tag) = prev_published {
        cmd.env("SHALE_PREV_PUBLISHED", tag);
    }
    let out = cmd.output().expect("bash runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{script} failed:\n{text}");
    assert!(!text.contains("not ok"), "{script}:\n{text}");
}

// spec: FORGE-011
#[test]
fn forge_011_test_land_script_passes() {
    run("scripts/test-land.sh");
}

// spec: FORGE-016
#[test]
fn forge_016_test_dev_metrics_script_passes() {
    run("scripts/test-dev-metrics.sh");
}

// spec: FORGE-013, FORGE-014
#[test]
fn forge_013_forge_014_test_release_script_passes() {
    run("scripts/test-release.sh");
}

// spec: BUILD-013, FORGE-014: scripts/test-release.sh unsets the variable itself, so a release that
// runs land.sh and its cargo test (which has it set in the environment) does not fail (#406)
#[test]
fn build_013_forge_014_release_scripts_ignore_inherited_prev_published() {
    run_with("scripts/test-release.sh", Some("v9.9.9"));
    run_with(".github/test-check-release.sh", Some("v9.9.9"));
}

// spec: BUILD-013
#[test]
fn build_013_test_check_release_script_passes() {
    run(".github/test-check-release.sh");
}

// spec: DEV-025
#[test]
fn dev_025_test_tex_env_script_passes() {
    run("scripts/test-tex-env.sh");
}

// spec: BUILD-035
#[test]
fn build_035_test_bench_wanted_script_passes() {
    run("scripts/test-bench-wanted.sh");
}

// spec: DEV-011
#[test]
fn dev_011_test_env_tests_script_passes() {
    run("scripts/test-env-tests.sh");
}

// spec: BUILD-025: BSD install (macOS) has no `-D` (create the leading folders), so the installer
// makes them with `mkdir -p` (#439)
#[test]
fn build_025_install_script_uses_no_gnu_only_install_d() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh")).unwrap();
    for l in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
        let words: Vec<&str> = l.split_whitespace().collect();
        let gnu = words.windows(2).any(|w| w[0] == "install" && w[1].starts_with('-') && !w[1].starts_with("--") && w[1].contains('D')) || l.contains("install --directory") || l.contains("install --create-leading");
        assert!(!gnu, "GNU-only `install -D` in scripts/install.sh: {l}");
    }
}

fn ci_job(name: &str) -> String {
    let ci = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".forgejo/workflows/ci.yml")).unwrap();
    let start = ci.find(&format!("\n  {name}:\n")).unwrap_or_else(|| panic!("no job {name}")) + 1;
    let rest = &ci[start + 1..];
    let end = rest
        .lines()
        .scan(0, |off, l| {
            let at = *off;
            *off += l.len() + 1;
            Some((at, l))
        })
        .skip(1)
        .find(|(_, l)| l.starts_with("  ") && !l.starts_with("   ") && !l.starts_with("  #") && l.trim_end().ends_with(':'))
        .map(|(at, _)| at)
        .unwrap_or(rest.len());
    ci[start..start + 1 + end].to_string()
}

// spec: DEV-011 (ADR-0026, #455): a tag, check or release, uses the cached TeX image; only the weekly
// scheduled run rebuilds it without the cache
#[test]
fn dev_011_only_the_weekly_run_refreshes_the_tex_image() {
    let env_tests = ci_job("env-tests");
    let line = env_tests.lines().find(|l| l.trim_start().starts_with("SHALE_ENV_TESTS_TEX_REFRESH:")).expect("the refresh variable is set");
    assert!(line.contains("github.event_name == 'schedule'"), "the weekly run refreshes");
    assert!(!line.contains("refs/tags") && !line.contains("ref_name"), "no tag refreshes: {line}");
    // a release tag still runs the real install and the template import (ADR-0021)
    let install = env_tests.lines().find(|l| l.trim_start().starts_with("SHALE_ENV_TESTS_TEX_INSTALL:")).unwrap();
    assert!(install.contains("!contains(github.ref_name, '-')"), "the install stays for release tags");
}

// spec: DEV-010 (#461): coverage runs for every tag push, check and release, because mirror needs it
// (that mirror needs all four jobs is build_016_mirror_needs_every_check_job_of_a_tag_push)
#[test]
fn dev_010_coverage_runs_for_every_tag_push() {
    let cov = ci_job("coverage");
    let cond = cov.lines().find(|l| l.trim_start().starts_with("if:")).expect("the coverage job has a condition");
    assert!(cond.contains("github.event_name != 'schedule'") && cond.contains("github.event_name != 'workflow_dispatch'"), "tag pushes only, not weekly, not by hand: {cond}");
    assert!(!cond.contains("contains(github.ref_name"), "check and release tags alike: {cond}");
}

// spec: FORGE-023 (#149): git's arguments show in the process list, so the tokens of the Forgejo jobs
// (the job token of every checkout, the GitHub token of `mirror`) go in git's environment configuration
#[test]
fn forge_023_no_workflow_puts_a_token_on_gits_command_line() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".forgejo/workflows");
    let mut headers = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for l in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(!l.contains("-c \"http") && !l.contains("-c http") && !l.contains("-c 'http") && !l.contains("--config"), "{name}: a git option with an http setting: {l}");
            if l.contains("extraHeader") {
                headers += 1;
                assert!(l.contains("GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http."), "{name}: the header is not set through git's environment: {l}");
            }
            if l.contains("GIT_CONFIG_VALUE_0=") {
                assert!(l.ends_with("git \"$@\"") && !l.contains("echo"), "{name}: the value is set for one git command only: {l}");
            }
        }
        assert!(!text.contains("set -x") && !text.contains("set -o xtrace"), "{path:?} traces commands, which would print a token");
    }
    // five checkouts and the fetch of the tags in ci.yml, the mirror's two functions, security.yml
    assert!(headers >= 8, "fewer header settings than expected: {headers}");
}
