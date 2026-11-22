//! Test support: a Word file checked against Microsoft's WordprocessingML schema with `xmllint`, as
//! `scripts/validate-docx.sh` does (DOCX-010, DEV-036). The tests never download: they read the cache that
//! `scripts/validate-docx.sh --fetch` fills (schemas that are not committed, their license is unclear) and
//! skip with a printed note when it, or xmllint, is missing (like pandoc, BUILD-018).

use super::package::Package;
use crate::pdf::tex::compile::WorkDir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

/// The commit of devoidfury/docx-validator whose schemas are used; the script has the same one.
pub const COMMIT: &str = "bfb31e310555a820f5f0813b0a2233ba5e9ca991";

/// How many error lines of a file are shown.
const SHOWN: usize = 8;

/// The result of a check.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Valid,
    /// Not checked: a tool or the schemas are missing (why).
    Skipped(String),
    /// The first errors, part by part.
    Invalid(String),
}

/// The cache folder of the schemas, `${XDG_CACHE_HOME:-~/.cache}/shale-dev/docx-schemas/<commit>`.
pub fn cache_dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match (xdg.filter(|x| !x.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(x), _) => PathBuf::from(x),
        (None, Some(h)) => Path::new(h).join(".cache"),
        (None, None) => return None,
    };
    Some(base.join("shale-dev/docx-schemas").join(COMMIT))
}

/// `wml-2010.xsd` of the cache when the script has fetched and verified all schemas into `cache`.
fn schema_in(cache: &Path) -> Option<PathBuf> {
    let xsd = cache.join("schemas/microsoft/wml-2010.xsd");
    (cache.join(".verified").is_file() && xsd.is_file()).then_some(xsd)
}

/// Checks the `word/*.xml` parts of a Word file (as bytes) against the schema in `cache`.
pub fn check_with(bytes: &[u8], cache: &Path) -> Outcome {
    let package = match Package::read(bytes) {
        Ok(p) => p,
        Err(e) => return Outcome::Invalid(format!("not a Word file: {e}")),
    };
    if Command::new("xmllint").arg("--version").output().is_err() {
        return Outcome::Skipped("xmllint is not installed here".into());
    }
    let Some(xsd) = schema_in(cache) else {
        return Outcome::Skipped(format!("the WordprocessingML schemas are not in {} (scripts/validate-docx.sh --fetch fetches them)", cache.display()));
    };
    let work = match WorkDir::new() {
        Ok(w) => w,
        Err(e) => return Outcome::Skipped(format!("no temporary folder: {e}")),
    };
    let parts: Vec<&str> = package.names().filter(|n| n.strip_prefix("word/").is_some_and(|rest| rest.ends_with(".xml") && !rest.contains('/'))).collect();
    let mut errors = vec![];
    for (i, name) in parts.iter().enumerate() {
        let text = match package.text(name) {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        let raw = work.path().join(format!("p{i}.xml"));
        if let Err(e) = work.write(&format!("p{i}.xml"), text.as_bytes()) {
            return Outcome::Skipped(format!("cannot write to the temporary folder: {e}"));
        }
        // formatted, so that an error names a line of a readable file
        let pretty = match Command::new("xmllint").arg("--format").arg(&raw).output() {
            Ok(o) if o.status.success() => o.stdout,
            Ok(o) => {
                errors.push(format!("{name}: not well-formed XML: {}", first_lines(&String::from_utf8_lossy(&o.stderr), 3)));
                continue;
            }
            Err(e) => return Outcome::Skipped(format!("xmllint cannot be started: {e}")),
        };
        let formatted = work.path().join(format!("p{i}.pretty.xml"));
        if let Err(e) = work.write(&format!("p{i}.pretty.xml"), &pretty) {
            return Outcome::Skipped(format!("cannot write to the temporary folder: {e}"));
        }
        match Command::new("xmllint").args(["--noout", "--nonet", "--schema"]).arg(&xsd).arg(&formatted).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let said = String::from_utf8_lossy(&o.stderr).replace("{http://schemas.openxmlformats.org/wordprocessingml/2006/main}", "w:");
                let said: String = said.lines().filter(|l| !l.ends_with("fails to validate")).collect::<Vec<_>>().join("\n");
                errors.push(format!("{name}: {}", first_lines(&said.replace(&format!("{}/", work.path().display()), ""), SHOWN)));
            }
            Err(e) => return Outcome::Skipped(format!("xmllint cannot be started: {e}")),
        }
    }
    if errors.is_empty() {
        Outcome::Valid
    } else {
        Outcome::Invalid(errors.join("\n"))
    }
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n    ")
}

/// Checks a Word file (as bytes) against the schemas of the developer's cache.
pub fn check(bytes: &[u8]) -> Outcome {
    let env = |name| std::env::var(name).ok();
    match cache_dir(env("XDG_CACHE_HOME").as_deref(), env("HOME").as_deref()) {
        Some(cache) => check_with(bytes, &cache),
        None => Outcome::Skipped("there is no cache folder (neither XDG_CACHE_HOME nor HOME)".into()),
    }
}

/// Panics naming `what` when the Word file does not validate; a skip prints its note once.
pub fn assert_valid(bytes: &[u8], what: &str) {
    static NOTE: Once = Once::new();
    match check(bytes) {
        Outcome::Valid => {}
        Outcome::Skipped(why) => NOTE.call_once(|| eprintln!("skipping: the schema check of the Word files is off: {why}")),
        Outcome::Invalid(errors) => panic!("{what} is not valid WordprocessingML:\n{errors}"),
    }
}
