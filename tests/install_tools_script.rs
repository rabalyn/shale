//! The architecture choice of `scripts/install-hugo.sh` and `scripts/install-pagefind.sh` (#683, SITE-070, SITE-048,
//! BUILD-018): the release asset follows the machine (the CI runner's job containers are arm64), and
//! an architecture without a download stops with a message. `--print-url` with `SHALE_ARCH` shows the
//! choice without downloading anything. Unix only: the scripts are POSIX `sh`.
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn run(script: &str, arch: &str) -> Output {
    Command::new("sh").arg(Path::new(ROOT).join("scripts").join(script)).arg("--print-url").env("SHALE_ARCH", arch).output().unwrap()
}

fn url(script: &str, arch: &str) -> String {
    let out = run(script, arch);
    assert!(out.status.success(), "{script} {arch}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn site_070_hugo_download_follows_the_architecture() {
    for arch in ["x86_64", "amd64"] {
        assert!(url("install-hugo.sh", arch).ends_with("_linux-amd64.tar.gz"), "{arch}");
    }
    for arch in ["aarch64", "arm64"] {
        assert!(url("install-hugo.sh", arch).ends_with("_linux-arm64.tar.gz"), "{arch}");
    }
}

#[test]
fn site_070_pagefind_download_follows_the_architecture() {
    for arch in ["x86_64", "amd64"] {
        assert!(url("install-pagefind.sh", arch).ends_with("-x86_64-unknown-linux-musl.tar.gz"), "{arch}");
    }
    for arch in ["aarch64", "arm64"] {
        assert!(url("install-pagefind.sh", arch).ends_with("-aarch64-unknown-linux-musl.tar.gz"), "{arch}");
    }
}

#[test]
fn site_070_an_unsupported_architecture_stops_and_names_it() {
    for script in ["install-hugo.sh", "install-pagefind.sh"] {
        let out = run(script, "riscv64");
        assert!(!out.status.success(), "{script}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("'riscv64'"), "{script}");
    }
}
