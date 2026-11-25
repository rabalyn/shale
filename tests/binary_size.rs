//! What keeps the executable small (BUILD-044): the user guide is embedded once.

use std::fs;
use std::path::{Path, PathBuf};

fn largest_file(dir: &Path, best: &mut Option<(u64, PathBuf)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue; // Shale's own folders of a vault, never bundled
        }
        if path.is_dir() {
            largest_file(&path, best);
        } else {
            let len = entry.metadata().unwrap().len();
            if best.as_ref().is_none_or(|(l, _)| len > *l) {
                *best = Some((len, path));
            }
        }
    }
}

/// How often `needle` occurs in `haystack` (a scan for its first byte, then a compare).
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    let mut n = 0;
    let mut from = 0;
    while let Some(i) = haystack[from..].iter().position(|b| *b == needle[0]) {
        let at = from + i;
        if haystack[at..].starts_with(needle) {
            n += 1;
        }
        from = at + 1;
    }
    n
}

// spec: BUILD-044
#[test]
fn build_044_the_user_guide_is_in_the_executable_once() {
    let mut best = None;
    largest_file(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/userguide"), &mut best);
    let (len, path) = best.expect("the guide has files");
    let media = fs::read(&path).unwrap();
    assert!(len > 100_000, "{} is not a real file (Git LFS not fetched?)", path.display());
    let exe = fs::read(env!("CARGO_BIN_EXE_shale")).unwrap();
    // a `const` table is copied into every codegen unit that uses it, so twice in the executable
    assert_eq!(count(&exe, &media), 1, "{} is in the executable not exactly once", path.display());
}
