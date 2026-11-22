//! `docs/screenshots/regen.sh` refuses a `shale` binary that is older than the sources (BUILD-019,
//! #85). Linux only: the script is bash and uses `find -newer` and `xvfb-run`.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

/// A stand-in for the program with this modification time.
fn fake_shale(tag: &str, modified: SystemTime) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-regen-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shale");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    path
}

fn regen(shale: &Path, args: &[&str]) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new("bash").arg(root.join("docs/screenshots/regen.sh")).args(args).env("SHALE", shale).env("SHALE_REGEN_DISPLAY", "host").output().expect("bash runs")
}

// spec: BUILD-019
#[test]
fn build_019_regen_refuses_a_binary_that_is_older_than_the_sources() {
    let old = fake_shale("old", SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000));
    let run = regen(&old, &["no-such-recipe"]);
    let err = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(1), "{err}");
    assert!(err.contains("is older than") && err.contains("build it again"), "{err}");
    let _ = std::fs::remove_dir_all(old.parent().unwrap());
}

// spec: BUILD-019
#[test]
fn build_019_regen_accepts_a_binary_that_is_newer_than_the_sources() {
    let new = fake_shale("new", SystemTime::now() + Duration::from_secs(3600));
    let run = regen(&new, &["no-such-recipe"]);
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(!err.contains("is older than"), "{err}");
    assert!(err.contains("no recipe named no-such-recipe"), "it got past the check of the age: {err}");
    let _ = std::fs::remove_dir_all(new.parent().unwrap());
}
