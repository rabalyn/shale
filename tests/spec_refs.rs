//! Every `// spec:` comment names spec rows that exist, and no row ID is used twice (DEV-014).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The marker, spelled in two parts so this file's own text is not taken for a comment.
const MARK: &str = concat!("// ", "spec:");

/// The IDs at the start of a `// spec:` comment: `ID` or `ID, ID, …`, read up to the first text
/// that is not an ID. `None` when the comment starts with no ID at all.
fn ids_in(comment: &str) -> Option<Vec<&str>> {
    let mut ids = Vec::new();
    let mut rest = comment.trim_start();
    loop {
        let letters = rest.find(|c: char| !c.is_ascii_uppercase()).unwrap_or(rest.len());
        let after = &rest[letters..];
        let digits = after.strip_prefix('-').map(|d| d.find(|c: char| !c.is_ascii_digit()).unwrap_or(d.len())).unwrap_or(0);
        if letters == 0 || digits == 0 {
            break;
        }
        let end = letters + 1 + digits;
        ids.push(&rest[..end]);
        match rest[end..].strip_prefix(',') {
            Some(next) => rest = next.trim_start(),
            None => break,
        }
    }
    (!ids.is_empty()).then_some(ids)
}

/// The IDs of the rule rows (`| ID | … |`) of a spec file.
fn rows_in(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter_map(|l| {
        let id = l.strip_prefix("| ")?.split(" |").next()?;
        ids_in(id).filter(|ids| ids.len() == 1 && ids[0] == id).map(|_| id)
    })
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

// spec: DEV-014
#[test]
fn dev_014_every_spec_comment_names_an_existing_row() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut specs = Vec::new();
    files(&root.join("docs/spec"), "md", &mut specs);
    let texts: Vec<String> = specs.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
    let rows: BTreeSet<&str> = texts.iter().flat_map(|t| rows_in(t)).collect();
    assert!(rows.contains("KEY-001") && rows.contains("DEV-014"), "the spec rows are read");

    let mut sources = Vec::new();
    files(&root.join("src"), "rs", &mut sources);
    files(&root.join("tests"), "rs", &mut sources);
    let mut bad = Vec::new();
    let mut seen = 0;
    for path in &sources {
        let text = fs::read_to_string(path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let Some(comment) = line.trim_start().strip_prefix(MARK) else { continue };
            let at = format!("{}:{}", path.strip_prefix(root).unwrap().display(), n + 1);
            match ids_in(comment) {
                None => bad.push(format!("{at}: no spec ID in `{}`", line.trim())),
                Some(ids) => {
                    for id in ids {
                        seen += 1;
                        if !rows.contains(id) {
                            bad.push(format!("{at}: {id} is no row of docs/spec/"));
                        }
                    }
                }
            }
        }
    }
    assert!(seen > 100, "the spec comments are found ({seen})");
    assert!(bad.is_empty(), "spec comments that name no row:\n{}", bad.join("\n"));
}

// spec: DEV-014
#[test]
fn dev_014_no_spec_row_id_is_used_twice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut specs = Vec::new();
    files(&root.join("docs/spec"), "md", &mut specs);
    specs.sort();
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut twice = Vec::new();
    for path in &specs {
        let text = fs::read_to_string(path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let Some(id) = rows_in(line).next() else { continue };
            let at = format!("{}:{}", path.strip_prefix(root).unwrap().display(), n + 1);
            match first.get(id) {
                Some(earlier) => twice.push(format!("{id} at {at}, already at {earlier}")),
                None => {
                    first.insert(id.to_string(), at);
                }
            }
        }
    }
    assert!(first.len() > 100, "the spec rows are read ({})", first.len());
    assert!(twice.is_empty(), "spec row IDs used twice:\n{}", twice.join("\n"));
}

// spec: DEV-014
#[test]
fn dev_014_the_ids_of_a_comment_are_read_up_to_the_free_text() {
    assert_eq!(ids_in(" KEY-003"), Some(vec!["KEY-003"]));
    assert_eq!(ids_in(" KEY-006, KEY-008, KEY-007"), Some(vec!["KEY-006", "KEY-008", "KEY-007"]));
    assert_eq!(ids_in(" SOC-019: cy's reply, and UTF-8"), Some(vec!["SOC-019"]));
    assert_eq!(ids_in(" VAULT-018 (the picker, MAIL-1)"), Some(vec!["VAULT-018"]));
    assert_eq!(ids_in(" EDIT-015, KEY-005 and more"), Some(vec!["EDIT-015", "KEY-005"]));
    assert_eq!(ids_in(" KEY003"), None);
    assert_eq!(ids_in(" key-003"), None);
    assert_eq!(ids_in(" KEY-"), None);
    assert_eq!(ids_in(""), None);

    let spec = "| ID | Rule |\n|---|---|\n| KEY-001 | One registry | IS NOW |\n| not a row |\nKEY-002 text\n";
    assert_eq!(rows_in(spec).collect::<Vec<_>>(), ["KEY-001"]);
}

/// The row IDs used more than once across `(file, text)` pairs, each with the files it is in.
fn duplicates<'a>(specs: &[(&'a str, &'a str)]) -> Vec<(&'a str, Vec<&'a str>)> {
    let mut found: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for (file, text) in specs {
        for id in rows_in(text) {
            found.entry(id).or_default().push(file);
        }
    }
    found.into_iter().filter(|(_, files)| files.len() > 1).collect()
}

// spec: DEV-023
#[test]
fn dev_023_every_spec_row_id_is_used_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut specs = Vec::new();
    files(&root.join("docs/spec"), "md", &mut specs);
    let texts: Vec<(String, String)> = specs.iter().map(|p| (p.strip_prefix(root).unwrap().display().to_string(), fs::read_to_string(p).unwrap())).collect();
    let pairs: Vec<(&str, &str)> = texts.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
    let rows = pairs.iter().map(|(_, t)| rows_in(t).count()).sum::<usize>();
    assert!(rows > 500, "the spec rows are read ({rows})");
    let dups = duplicates(&pairs);
    let list: Vec<String> = dups.iter().map(|(id, files)| format!("{id}: {}", files.join(", "))).collect();
    assert!(dups.is_empty(), "spec row IDs used more than once:\n{}", list.join("\n"));
}

// spec: DEV-023
#[test]
fn dev_023_duplicates_are_found_across_files() {
    let a = "| EDIT-024 | Pictures | IS NOW |\n| EDIT-025 | Captions | IS NOW |\n| EDIT-024 | Tables | IS NOW |\n";
    let b = "| EDIT-025 | Table buttons | IS NOW |\n| KEY-001 | Keys | IS NOW |\n";
    assert_eq!(duplicates(&[("a.md", a), ("b.md", b)]), [("EDIT-024", vec!["a.md", "a.md"]), ("EDIT-025", vec!["a.md", "b.md"])]);
    assert!(duplicates(&[("b.md", b)]).is_empty());
}
