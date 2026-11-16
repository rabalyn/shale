//! The list of tests that `scripts/cross-check.sh --wine` skips (DEV-015) stays true: every entry
//! is a test that exists, with a reason, so a renamed or deleted test does not leave a stale skip.

use std::fs;
use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(fs::read_to_string(&p).unwrap());
        }
    }
}

/// `#[test]` followed by `fn <name>(` in one of the sources.
fn is_test(srcs: &[String], name: &str) -> bool {
    let f = format!("fn {name}(");
    srcs.iter().any(|s| {
        let lines: Vec<&str> = s.lines().map(str::trim).collect();
        lines.iter().enumerate().any(|(i, l)| l.starts_with(&f) && lines[..i].iter().rev().take_while(|p| p.starts_with("#[") || p.starts_with("//")).any(|p| *p == "#[test]"))
    })
}

// spec: DEV-015
#[test]
fn dev_015_every_known_wine_failure_is_a_test_that_exists_and_has_a_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut srcs = vec![];
    sources(&root.join("src"), &mut srcs);
    let list = fs::read_to_string(root.join("scripts/wine-known-failures.txt")).unwrap();
    let mut n = 0;
    for line in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (name, reason) = line.split_once('#').unwrap_or((line, ""));
        let name = name.trim();
        assert!(!reason.trim().is_empty(), "no reason after `#` for {name}");
        assert!(name.contains("::") && !name.contains(char::is_whitespace), "not a full test name: {line}");
        let last = name.rsplit("::").next().unwrap();
        assert!(is_test(&srcs, last), "{name} in scripts/wine-known-failures.txt is not a test in src/");
        n += 1;
    }
    assert!(n > 0, "the list has no entries");
    // the check tells a test from a plain function
    assert!(!is_test(&srcs, "write_atomic"), "write_atomic is no test");
}
