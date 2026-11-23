//! A long-lived secret is a `Secret`, not a `String` (SEC-016): no field of production code that
//! is named like a password, token or client secret has the type `String`.

use std::fs;
use std::path::{Path, PathBuf};

/// Field names that hold a secret.
const NAMES: [&str; 13] = ["password", "passwd", "token", "access_token", "refresh_token", "client_secret", "app_secret", "app_password", "secret", "access_jwt", "refresh_jwt", "session_pw", "passphrase"];

/// The files that may hold such a field as a `String`, each with its reason: none today.
const ALLOWED: [&str; 0] = [];

/// The production code of a source file: what comes before its `#[cfg(test)]` module.
fn production(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or("")
}

/// Whether a file under `src/` is test code as a whole (`x_tests.rs`, `tests.rs`, `tests/…`, `ui_test_support.rs`, the pretend servers).
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.ends_with("tests.rs") || name.starts_with("ui_test") || name.starts_with("fake_") || name.ends_with("_fake.rs") || rel.contains("/tests/")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The field a line declares (`pub password: String,`) when its name is a secret's and its type a
/// `String` (also `Option<String>`).
fn string_secret_field(line: &str) -> Option<&str> {
    let l = line.trim_start().trim_start_matches("pub(crate) ").trim_start_matches("pub ");
    let (name, ty) = l.split_once(':')?;
    let ty = ty.trim().trim_end_matches(',');
    (NAMES.contains(&name) && matches!(ty, "String" | "Option<String>")).then_some(name)
}

// spec: SEC-016
#[test]
fn sec_016_no_field_of_production_code_holds_a_password_or_token_as_a_string() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    files.sort();
    let mut bad = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if is_test_file(&rel) || ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let text = fs::read_to_string(f).unwrap();
        for (n, line) in production(&text).lines().enumerate() {
            if let Some(name) = string_secret_field(line) {
                bad.push(format!("{rel}:{}: {name} is a String; hold it in a `Secret` (secret.rs, SEC-016)", n + 1));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// spec: SEC-016
#[test]
fn sec_016_the_check_sees_a_string_field_and_not_a_secret_one() {
    assert_eq!(string_secret_field("    pub password: String,"), Some("password"));
    assert_eq!(string_secret_field("    token: Option<String>,"), Some("token"));
    assert_eq!(string_secret_field("    pub(crate) refresh_token: String,"), Some("refresh_token"));
    assert_eq!(string_secret_field("    pub password: Secret,"), None);
    assert_eq!(string_secret_field("    pub token: Option<Secret>,"), None);
    assert_eq!(string_secret_field("    pub username: String,"), None);
}
