//! No test starts the machine's GnuPG (DEV-017): `Gnupg::system()` is for the app itself and the test kit.

use std::fs;
use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

// spec: DEV-017
#[test]
fn dev_017_the_machines_gnupg_is_named_only_where_the_app_and_the_test_home_are_made() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    assert!(files.iter().any(|p| p.ends_with("mail/gpg.rs")), "the sources are read");
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let uses = text.lines().filter(|l| l.contains("Gnupg::system(") && !l.trim_start().starts_with("//")).count();
        let allowed = if path.ends_with("mail/gpg.rs") { 3 } else { 0 }; // `Default`, `TestHome` and the guard test of gpg.rs
        assert!(uses <= allowed, "{} names the machine's GnuPG {uses} times; a test must use `Gnupg::default()` or a `TestHome`", path.display());
    }
}
