//! The shell tests of the landing and tag scripts run with `cargo test`, so `scripts/land.sh`
//! checks them before every landing (FORGE-011, FORGE-013, FORGE-014, ADR-0018): Forgejo's CI runs
//! only for tags now. Linux only: the scripts use GNU `sed -i` and `flock`.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

fn run(script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("bash").arg(root.join(script)).current_dir(root).env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("LAND_CHECK_CMD").output().expect("bash runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{script} failed:\n{text}");
    assert!(!text.contains("not ok"), "{script}:\n{text}");
}

// spec: FORGE-011
#[test]
fn forge_011_test_land_script_passes() {
    run("scripts/test-land.sh");
}

// spec: FORGE-013, FORGE-014
#[test]
fn forge_013_forge_014_test_release_script_passes() {
    run("scripts/test-release.sh");
}
