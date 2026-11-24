//! Every `// spec:` comment names spec rows that exist, no row ID is used twice (DEV-014, DEV-023),
//! every test an IS NOW row names exists (DEV-033) and carries the row's ID (DEV-039).

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

// ---- DEV-033: the tests a spec names exist ----

/// The words a Test cell may start with when it names no test (a rule checked by a process, by CI
/// where it cannot run locally, or not tested at all). Anything else without a test reference is
/// a row to fix: name its test, or start the cell with one of these.
const NO_TEST_WORDS: [&str; 3] = ["process", "CI", "untested"];

/// A test reference found in a Test cell.
#[derive(Debug, PartialEq)]
enum TestRef {
    /// `path.rs::name` or `path.rs::module::…::name`: the file as written, and the last segment.
    /// A last segment ending in `*` (`path.rs::pointer_*`) is a prefix: some function starts with it.
    InFile { file: String, name: String, prefix: bool },
    /// A script or a test file as a whole (`tests/x.rs`, `scripts/test-y.sh`): it exists.
    File(String),
    /// A bare `snake_case` name: a function, or the prefix of some (a `cargo test` filter).
    Bare(String),
}

/// The test references of a Test cell: only backticked tokens count. A token with `::` whose
/// first part ends in `.rs` is a path reference (`path.rs::name`, `path.rs::name_prefix*`); a token
/// without `::` that is a path ending in `.rs` or `.sh` is a file reference; a token of lowercase
/// letters, digits and `_` with at least one `_` is a bare name. Everything else (a folder, a
/// command line, a type, a program) is not a reference and is ignored.
fn test_refs(cell: &str) -> Vec<TestRef> {
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let file_name = |s: &str| (s.ends_with(".rs") || s.ends_with(".sh")) && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c));
    let mut refs = Vec::new();
    for token in cell.split('`').skip(1).step_by(2) {
        if let Some((file, rest)) = token.split_once("::") {
            let parts: Vec<&str> = rest.split("::").collect();
            let last = parts[parts.len() - 1];
            let (name, prefix) = last.strip_suffix('*').map_or((last, false), |n| (n, true));
            if file.ends_with(".rs") && parts[..parts.len() - 1].iter().all(|p| ident(p)) && ident(name) {
                refs.push(TestRef::InFile { file: file.to_string(), name: name.to_string(), prefix });
            }
        } else if file_name(token) {
            refs.push(TestRef::File(token.to_string()));
        } else if token.starts_with(|c: char| c.is_ascii_lowercase()) && token.contains('_') && token.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            refs.push(TestRef::Bare(token.to_string()));
        }
    }
    refs
}

/// Whether a Test cell starts with one of the `NO_TEST_WORDS`.
fn starts_with_no_test_word(cell: &str) -> bool {
    let word: String = cell.trim_start().chars().take_while(|c| c.is_alphanumeric()).collect();
    NO_TEST_WORDS.contains(&word.as_str())
}

/// The `(ID, Test cell)` of every IS NOW row (the columns are `ID | Rule | Status | Code | Test`,
/// read from the right so a ` | ` in the rule does not matter).
fn is_now_rows(text: &str) -> Vec<(&str, &str)> {
    text.lines()
        .filter_map(|line| {
            let id = rows_in(line).next()?;
            let mut cells = line.strip_suffix(" |")?.rsplitn(4, " | ");
            let (test, _code, status) = (cells.next()?, cells.next()?, cells.next()?);
            status.starts_with("IS NOW").then_some((id, test))
        })
        .collect()
}

/// The `.rs` files of `src/` and `tests/` and the `.sh` scripts of the repo as `(path from the
/// repo root, text)`.
fn source_tree(root: &Path) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    files(&root.join("src"), "rs", &mut paths);
    files(&root.join("tests"), "rs", &mut paths);
    for dir in ["scripts", ".forgejo", ".github", "docs"] {
        files(&root.join(dir), "sh", &mut paths);
    }
    paths.sort();
    paths.iter().map(|p| (p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), fs::read_to_string(p).unwrap())).collect()
}

/// Whether `text` defines `<keyword> <name>` (`fn` or `mod`) as a whole word.
fn defines(text: &str, keyword: &str, name: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let needle = format!("{keyword} {name}");
    text.match_indices(&needle).any(|(at, _)| !text[..at].ends_with(is_ident) && !text[at + needle.len()..].starts_with(is_ident))
}

/// The directory part of a path, and `a/b/../c` folded to `a/c`.
fn join_path(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The files that make up the module a file declares: the file itself, the files below its module
/// folder (`x.rs` ⇒ `x/…`; `x/mod.rs`, `main.rs`, `lib.rs` ⇒ their own folder) and the files its
/// `#[path = "…"]` attributes pull in.
fn module_files<'a>(tree: &'a [(String, String)], file: &str) -> Vec<&'a (String, String)> {
    let (dir, name) = file.rsplit_once('/').unwrap_or(("", file));
    let folder = match name {
        "mod.rs" | "main.rs" | "lib.rs" => format!("{dir}/"),
        _ => format!("{dir}/{}/", name.trim_end_matches(".rs")),
    };
    let own = tree.iter().find(|(p, _)| p == file);
    let mut out: Vec<&(String, String)> = own.into_iter().collect();
    out.extend(tree.iter().filter(|(p, _)| p != file && p.starts_with(&folder)));
    if let Some((_, text)) = own {
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix("#[path = \"") else { continue };
            let Some((rel, _)) = rest.split_once('"') else { continue };
            let target = join_path(dir, rel);
            out.extend(tree.iter().filter(|(p, _)| *p == target));
        }
    }
    out
}

/// The files a path of a Test cell names: every file whose path is it or ends with `/` and it
/// (`tests/x.rs`, `scripts/y.sh`, `tex/book.rs` ⇒ `src/pdf/tex/book.rs`), with `x/mod.rs` for `x.rs`.
/// A name shared by two folders (`format.rs`) names both.
fn named_files<'a>(tree: &'a [(String, String)], file: &str) -> Vec<&'a str> {
    let as_mod = file.strip_suffix(".rs").map(|stem| format!("{stem}/mod.rs"));
    tree.iter().filter(|(p, _)| [Some(file), as_mod.as_deref()].into_iter().flatten().any(|n| p == n || p.ends_with(&format!("/{n}")))).map(|(p, _)| p.as_str()).collect()
}

/// Whether a test is named by a line of its own in a macro invocation (`name: "pub", En;`) in a
/// file that defines a `macro_rules!`: the macro writes its `fn`.
fn defined_by_macro(text: &str, name: &str) -> bool {
    text.contains("macro_rules!") && text.lines().any(|l| l.trim_start().strip_prefix(name).is_some_and(|rest| rest.starts_with(':')))
}

/// Whether `text` has a `fn` whose name starts with `start`.
fn has_fn_starting(text: &str, start: &str) -> bool {
    text.match_indices("fn ").any(|(at, _)| !text[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') && text[at + 3..].starts_with(start))
}

/// `Ok` when the reference resolves, else what is wrong: `file missing`, or `no fn or mod <name>
/// in <files>`. A path reference needs `fn <name>` or `mod <name>` in the named file or the module
/// it declares (`name*`: a `fn` starting with `name`); a file reference needs the file; a bare name
/// needs `fn <name>` anywhere in the tree, or some `fn` starting with it.
fn resolve(tree: &[(String, String)], reference: &TestRef) -> Result<(), String> {
    match reference {
        TestRef::File(file) => {
            if named_files(tree, file).is_empty() {
                Err("file missing".to_string())
            } else {
                Ok(())
            }
        }
        TestRef::InFile { file, name, prefix } => {
            let named = named_files(tree, file);
            if named.is_empty() {
                return Err("file missing".to_string());
            }
            let searched: Vec<&(String, String)> = named.iter().flat_map(|f| module_files(tree, f)).collect();
            let found = |text: &str| if *prefix { has_fn_starting(text, name) } else { defines(text, "fn", name) || defines(text, "mod", name) || defined_by_macro(text, name) };
            if searched.iter().any(|(_, text)| found(text)) {
                return Ok(());
            }
            let star = if *prefix { "*" } else { "" };
            Err(format!("no fn or mod {name}{star} in {}", named.join(", ")))
        }
        TestRef::Bare(name) => {
            if tree.iter().any(|(_, text)| has_fn_starting(text, name)) {
                Ok(())
            } else {
                Err(format!("no fn named or starting with {name} in src/ or tests/"))
            }
        }
    }
}

/// The problems of the Test cells of the IS NOW rows in `(file, text)` spec files, one line each.
fn unresolved_tests(tree: &[(String, String)], specs: &[(&str, &str)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (_, text) in specs {
        for (id, cell) in is_now_rows(text) {
            let refs = test_refs(cell);
            if refs.is_empty() && !starts_with_no_test_word(cell) {
                bad.push(format!("{id}: no test named and the cell starts with none of {NO_TEST_WORDS:?}: {}", cell.chars().take(60).collect::<String>()));
            }
            for reference in refs {
                if let Err(why) = resolve(tree, &reference) {
                    let shown = match &reference {
                        TestRef::InFile { file, name, prefix } => format!("{file}::{name}{}", if *prefix { "*" } else { "" }),
                        TestRef::File(file) | TestRef::Bare(file) => file.clone(),
                    };
                    bad.push(format!("{id}: {shown} ({why})"));
                }
            }
        }
    }
    bad.sort();
    bad
}

// ---- DEV-039: the tests a spec names are about the row ----

/// Whether the name `name` carries the row `id`: `build_019_…` for `BUILD-019`, in any case.
fn name_carries(name: &str, id: &str) -> bool {
    let (name, id) = (name.to_ascii_lowercase(), id.to_ascii_lowercase().replace('-', "_"));
    name.match_indices(&id).any(|(at, _)| !name[..at].ends_with(|c: char| c.is_ascii_alphanumeric()) && !name[at + id.len()..].starts_with(|c: char| c.is_ascii_digit()))
}

/// The IDs of the `// spec:` comments in the block of attributes and comments right above line `at`.
fn ids_above(lines: &[&str], at: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for line in lines[..at].iter().rev() {
        let l = line.trim_start();
        if let Some(comment) = l.strip_prefix(MARK) {
            ids.extend(ids_in(comment).unwrap_or_default().into_iter().map(str::to_string));
        } else if !(l.starts_with("#[") || l.starts_with("//")) {
            break;
        }
    }
    ids
}

/// The lines of `text` where `<keyword> <name>` is defined (`fn`, `mod`), or, when `name` is the
/// start of a function (`prefix`), where a `fn` starting with it is.
fn definition_lines(text: &str, keyword: &str, name: &str, prefix: bool) -> Vec<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let needle = format!("{keyword} {name}");
    text.lines().enumerate().filter(|(_, line)| line.match_indices(&needle).any(|(at, _)| !line[..at].ends_with(is_ident) && (prefix || !line[at + needle.len()..].starts_with(is_ident)))).map(|(n, _)| n).collect()
}

/// The name a definition line defines (`pub fn name(…)`, `mod name {`, a macro line `name: …`).
fn defined_name(line: &str) -> &str {
    let l = line.trim_start();
    let l = l.split_once("fn ").or_else(|| l.split_once("mod ")).map_or(l, |(_, rest)| rest);
    &l[..l.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(l.len())]
}

/// Whether the definitions of a reference in `text` carry the row `id`: the name has it, or a
/// `// spec:` comment above lists it. For a macro that writes the function the line with the name
/// counts.
fn definition_carries(text: &str, reference: &TestRef, id: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let (name, prefix) = match reference {
        TestRef::InFile { name, prefix, .. } => (name.as_str(), *prefix),
        TestRef::Bare(name) => (name.as_str(), true),
        TestRef::File(_) => return false,
    };
    let mut at: Vec<usize> = definition_lines(text, "fn", name, prefix);
    if !prefix {
        at.extend(definition_lines(text, "mod", name, false));
        if defined_by_macro(text, name) {
            at.extend(lines.iter().enumerate().filter(|(_, l)| l.trim_start().strip_prefix(name).is_some_and(|rest| rest.starts_with(':'))).map(|(n, _)| n));
        }
    }
    at.iter().any(|&n| name_carries(defined_name(lines[n]), id) || ids_above(&lines, n).iter().any(|i| i == id))
}

/// Whether a text mentions the row `id` (in a comment or a name), for a whole file or script.
fn mentions(text: &str, id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let lower = text.to_ascii_lowercase();
    [id.clone(), id.replace('-', "_")].iter().any(|form| lower.match_indices(form.as_str()).any(|(at, _)| !lower[..at].ends_with(|c: char| c.is_ascii_alphanumeric()) && !lower[at + form.len()..].starts_with(|c: char| c.is_ascii_digit())))
}

/// Whether the test a resolved reference names carries the row `id` (see `definition_carries`; a
/// whole file or script mentions it somewhere).
fn carries_id(tree: &[(String, String)], reference: &TestRef, id: &str) -> bool {
    match reference {
        TestRef::File(file) => named_files(tree, file).iter().any(|f| tree.iter().any(|(p, text)| p == f && mentions(text, id))),
        TestRef::InFile { file, .. } => named_files(tree, file).iter().flat_map(|f| module_files(tree, f)).any(|(_, text)| definition_carries(text, reference, id)),
        TestRef::Bare(_) => tree.iter().any(|(_, text)| definition_carries(text, reference, id)),
    }
}

/// The references of IS NOW rows that resolve (DEV-033) but are not about their row: one line each,
/// `ID: reference`. References that do not resolve are left to `unresolved_tests`.
fn tests_without_their_id(tree: &[(String, String)], specs: &[(&str, &str)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (_, text) in specs {
        for (id, cell) in is_now_rows(text) {
            for reference in test_refs(cell) {
                if resolve(tree, &reference).is_err() {
                    continue;
                }
                if !carries_id(tree, &reference, id) {
                    let shown = match &reference {
                        TestRef::InFile { file, name, prefix } => format!("{file}::{name}{}", if *prefix { "*" } else { "" }),
                        TestRef::File(file) | TestRef::Bare(file) => file.clone(),
                    };
                    bad.push(format!("{id}: {shown}"));
                }
            }
        }
    }
    bad.sort();
    bad.dedup();
    bad
}

/// The exceptions of DEV-039 that exist today: tests a row names that carry no ID of it yet, one
/// `ID: reference` per line (`#` starts a comment). The list may only shrink.
const ALLOWLIST: &str = "tests/spec_refs_allowlist.txt";

/// The number of entries of `ALLOWLIST`. Lower it when you remove entries; never raise it.
const ALLOWLIST_LEN: usize = 111;

/// The entries of an allowlist text.
fn allowlist_entries(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

// spec: DEV-039
#[test]
fn dev_039_every_test_a_spec_row_names_is_about_that_row() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    files(&root.join("docs/spec"), "md", &mut paths);
    paths.sort();
    let texts: Vec<(String, String)> = paths.iter().map(|p| (p.display().to_string(), fs::read_to_string(p).unwrap())).collect();
    let specs: Vec<(&str, &str)> = texts.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
    let tree = source_tree(root);
    let found = tests_without_their_id(&tree, &specs);
    let allow_text = fs::read_to_string(root.join(ALLOWLIST)).unwrap();
    let allowed = allowlist_entries(&allow_text);
    let new: Vec<&String> = found.iter().filter(|f| !allowed.contains(&f.as_str())).collect();
    let how = "A spec row names a test that is not about that row. Fix it in the test, not in the list: add a comment line `// spec: <ID>` right above the test (above its `#[test]`; several IDs: `// spec: ID1, ID2`), or give the test a name that starts with or contains the ID (`build_019_…`). Where the line says a file or a script, mention the ID in its header comment. If the test really belongs to another rule, name a better test in the row's Test column instead. Do not add to tests/spec_refs_allowlist.txt: it only shrinks (DEV-039)";
    assert!(new.is_empty(), "tests that spec rows name and that do not carry the row's ID ({}), as `ID: reference`.\n{how}.\n{}", new.len(), new.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
    let fixed: Vec<&&str> = allowed.iter().filter(|a| !found.iter().any(|f| f == **a)).collect();
    assert!(fixed.is_empty(), "entries of tests/spec_refs_allowlist.txt that are no exception any more (remove them and lower ALLOWLIST_LEN):\n{}", fixed.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n"));
    assert_eq!(allowed.len(), ALLOWLIST_LEN, "ALLOWLIST_LEN says how long the list is: lower it with the list, never raise it");
}

// spec: DEV-039
#[test]
fn dev_039_a_test_carries_its_row_by_name_or_by_a_spec_comment_above_it() {
    let tree: Vec<(String, String)> = [
        (
            "src/a.rs",
            "// spec: KEY-001\n#[test]\nfn by_comment() {}\n\n\
             // spec: KEY-002, build_019_is_text, BUILD-019 free text\n#[test]\n/// docs\nfn by_a_list_with_text() {}\n\n\
             // spec: KEY-001\n\nfn comment_is_not_above() {}\n\n\
             #[test]\nfn build_019_by_name() {}\n\n#[test]\nfn xbuild_019_is_no_match() {}\n\n#[test]\nfn build_0190_is_no_match() {}\n\n\
             // spec: MAIL-001\nmod parts {\n}\n\n\
             macro_rules! cases { () => {}; }\ncases! {\n    // spec: NAV-001\n    made_by_a_macro: \"x\";\n}\n",
        ),
        ("src/b.rs", "// spec: KEY-001\nfn shared_name() {}\nfn shared_name_other() {}\n"),
        ("scripts/test-y.sh", "# FORGE-020: the script\n"),
    ]
    .iter()
    .map(|(p, t)| (p.to_string(), t.to_string()))
    .collect();
    let file = |n: &str| TestRef::InFile { file: "a.rs".into(), name: n.into(), prefix: false };
    let ok = |r: &TestRef, id: &str| carries_id(&tree, r, id);
    assert!(ok(&file("by_comment"), "KEY-001"));
    assert!(!ok(&file("by_comment"), "KEY-002"), "another row's ID is not enough");
    assert!(ok(&file("by_a_list_with_text"), "KEY-002") && !ok(&file("by_a_list_with_text"), "BUILD-019"), "the IDs of a comment end where its text starts");
    assert!(!ok(&file("comment_is_not_above"), "KEY-001"), "a blank line ends the block above the test");
    assert!(ok(&file("build_019_by_name"), "BUILD-019") && ok(&file("build_019_by_name"), "build-019"), "the name, in any case");
    assert!(!ok(&file("xbuild_019_is_no_match"), "BUILD-019") && !ok(&file("build_0190_is_no_match"), "BUILD-019"), "a whole ID, not a part of a word or a number");
    assert!(ok(&file("parts"), "MAIL-001"), "a module has it on its mod line");
    assert!(ok(&file("made_by_a_macro"), "NAV-001"), "a test a macro writes has it above its line");
    let prefix = TestRef::InFile { file: "b.rs".into(), name: "shared_name".into(), prefix: true };
    assert!(ok(&prefix, "KEY-001"), "one function of the prefix is enough");
    assert!(ok(&TestRef::Bare("by_comment".into()), "KEY-001") && !ok(&TestRef::Bare("by_comment".into()), "KEY-009"));
    assert!(ok(&TestRef::File("scripts/test-y.sh".into()), "FORGE-020") && !ok(&TestRef::File("scripts/test-y.sh".into()), "FORGE-02"), "a file mentions the ID");
    let spec = "| KEY-001 | Rule | IS NOW | `a.rs` | `a.rs::by_comment`, `a.rs::build_019_by_name` |\n| KEY-001 | Rule | SHOULD BE | `a.rs` | `a.rs::build_019_by_name` |\n";
    assert_eq!(tests_without_their_id(&tree, &[("s.md", spec)]), ["KEY-001: a.rs::build_019_by_name"], "only IS NOW rows count, and a name that does not resolve is left to DEV-033");
}

// spec: DEV-039
#[test]
fn dev_039_the_allowlist_is_read_without_comments() {
    assert_eq!(allowlist_entries("# a comment\n\nKEY-001: a.rs::b\n  MAIL-002: c.rs\n"), ["KEY-001: a.rs::b", "MAIL-002: c.rs"]);
    let listed = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(ALLOWLIST)).unwrap();
    let entries = allowlist_entries(&listed);
    let mut sorted = entries.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(entries, sorted, "the list is sorted, without doubles");
    assert!(entries.iter().all(|e| e.split_once(": ").is_some_and(|(id, _)| ids_in(id).is_some())), "every entry is `ID: reference`");
}

// spec: DEV-033
#[test]
fn dev_033_every_test_a_spec_row_names_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    files(&root.join("docs/spec"), "md", &mut paths);
    paths.sort();
    let texts: Vec<(String, String)> = paths.iter().map(|p| (p.display().to_string(), fs::read_to_string(p).unwrap())).collect();
    let specs: Vec<(&str, &str)> = texts.iter().map(|(f, t)| (f.as_str(), t.as_str())).collect();
    let rows = specs.iter().map(|(_, t)| is_now_rows(t).len()).sum::<usize>();
    assert!(rows > 500, "the IS NOW rows are read ({rows})");
    let tree = source_tree(root);
    assert!(tree.len() > 100, "the sources are read ({})", tree.len());
    let bad = unresolved_tests(&tree, &specs);
    assert!(bad.is_empty(), "tests that spec rows name and that do not exist ({}):\n{}", bad.len(), bad.join("\n"));
}

#[cfg(test)]
fn fixture_tree() -> Vec<(String, String)> {
    [
        ("src/media.rs", "mod engine;\nfn helper() {}\n#[test]\nfn plays_a_clip() {}\nmod fake { fn nested_case() {} }\n"),
        ("src/media/macros.rs", "macro_rules! cases { () => {}; }\ncases! {\n    made_by_a_macro: \"x\";\n}\n"),
        ("src/media/engine.rs", "#[test]\nfn the_engine_starts() {}\n"),
        ("src/pdf/tex/mod.rs", "#[path = \"../extra_tests.rs\"]\nmod extra_tests;\n"),
        ("src/pdf/tex/book.rs", "fn books_are_bound() {}\n"),
        ("src/pdf/extra_tests.rs", "fn pulled_in_by_path() {}\n"),
        ("scripts/test-y.sh", "echo\n"),
        ("tests/spec_x.rs", "fn dev_999_first_case() {}\nfn dev_999_second_case() {}\n"),
    ]
    .iter()
    .map(|(p, t)| (p.to_string(), t.to_string()))
    .collect()
}

// spec: DEV-033
#[test]
fn dev_033_references_are_read_from_a_test_cell() {
    let file = |f: &str, n: &str| TestRef::InFile { file: f.to_string(), name: n.to_string(), prefix: false };
    assert_eq!(test_refs("`media.rs::plays_a_clip`"), [file("media.rs", "plays_a_clip")]);
    assert_eq!(test_refs("`media.rs::fake::nested_case`, `tests/spec_x.rs::a_b`"), [file("media.rs", "nested_case"), file("tests/spec_x.rs", "a_b")]);
    assert_eq!(test_refs("`caldav_env`, `dev_025`"), [TestRef::Bare("caldav_env".into()), TestRef::Bare("dev_025".into())]);
    assert_eq!(test_refs("`pointer.rs::pointer_*`"), [TestRef::InFile { file: "pointer.rs".into(), name: "pointer_".into(), prefix: true }]);
    assert_eq!(test_refs("`tests/spec_x.rs`, `scripts/test-y.sh`"), [TestRef::File("tests/spec_x.rs".into()), TestRef::File("scripts/test-y.sh".into())]);
    // not references: a folder, a command line, a type, a bare glob, a plain word, text outside backticks
    assert!(test_refs("`docs/perf/`, `cargo test -- --ignored mail_env`, `ShaleApp`, `dev_026_*`, `coverage`, `Cargo.toml`, plain_text").is_empty());
    assert!(test_refs("`ShaleApp::run`, `a.rs::bad-name`, `import::tb_store::fixture_tests::*`").is_empty());
}

// spec: DEV-033
#[test]
fn dev_033_a_path_resolves_in_its_file_its_child_files_and_a_path_attribute() {
    let tree = fixture_tree();
    let file = |f: &str, n: &str| TestRef::InFile { file: f.to_string(), name: n.to_string(), prefix: false };
    assert_eq!(resolve(&tree, &file("media.rs", "plays_a_clip")), Ok(()));
    assert_eq!(resolve(&tree, &file("media.rs", "nested_case")), Ok(()), "a function of an inline module");
    assert_eq!(resolve(&tree, &file("media.rs", "fake")), Ok(()), "a module as the last part");
    assert_eq!(resolve(&tree, &file("media.rs", "the_engine_starts")), Ok(()), "a child file of media.rs");
    assert_eq!(resolve(&tree, &file("media.rs", "made_by_a_macro")), Ok(()), "a test a macro writes");
    assert!(resolve(&tree, &file("media.rs", "helper_typo")).is_err());
    assert_eq!(resolve(&tree, &file("pdf/tex.rs", "books_are_bound")), Ok(()), "tex/mod.rs for a missing tex.rs");
    assert_eq!(resolve(&tree, &file("pdf/tex/mod.rs", "pulled_in_by_path")), Ok(()), "a file named by #[path]");
    assert_eq!(resolve(&tree, &file("tex/book.rs", "books_are_bound")), Ok(()), "a path matched by its end");
    assert_eq!(resolve(&tree, &file("tests/spec_x.rs", "dev_999_second_case")), Ok(()));
    assert_eq!(resolve(&tree, &file("engine.rs", "plays_a_clip")), Err("no fn or mod plays_a_clip in src/media/engine.rs".to_string()));
    assert_eq!(resolve(&tree, &file("media.rs", "plays_a_clip_typo")), Err("no fn or mod plays_a_clip_typo in src/media.rs".to_string()));
    assert_eq!(resolve(&tree, &file("gone.rs", "plays_a_clip")), Err("file missing".to_string()));
    assert!(resolve(&tree, &file("media.rs", "play")).is_err(), "a prefix is no match for a path reference");
    let star = |f: &str, n: &str| TestRef::InFile { file: f.to_string(), name: n.to_string(), prefix: true };
    assert_eq!(resolve(&tree, &star("tests/spec_x.rs", "dev_999_")), Ok(()), "`file.rs::name_*`");
    assert_eq!(resolve(&tree, &star("tests/spec_x.rs", "dev_998_")), Err("no fn or mod dev_998_* in tests/spec_x.rs".to_string()));
    assert_eq!(resolve(&tree, &TestRef::File("scripts/test-y.sh".into())), Ok(()));
    assert_eq!(resolve(&tree, &TestRef::File("tex/book.rs".into())), Ok(()));
    assert_eq!(resolve(&tree, &TestRef::File("scripts/test-gone.sh".into())), Err("file missing".to_string()));
}

// spec: DEV-033
#[test]
fn dev_033_a_bare_name_is_a_function_or_the_start_of_one() {
    let tree = fixture_tree();
    assert_eq!(resolve(&tree, &TestRef::Bare("plays_a_clip".into())), Ok(()));
    assert_eq!(resolve(&tree, &TestRef::Bare("dev_999".into())), Ok(()), "a cargo test filter");
    assert_eq!(resolve(&tree, &TestRef::Bare("dev_999_second".into())), Ok(()));
    assert!(resolve(&tree, &TestRef::Bare("dev_998".into())).is_err());
    assert!(resolve(&tree, &TestRef::Bare("lays_a_clip".into())).is_err(), "the start of the name, not a part of it");
}

// spec: DEV-033
#[test]
fn dev_033_a_cell_without_a_test_starts_with_an_allowed_word() {
    assert!(starts_with_no_test_word("process"));
    assert!(starts_with_no_test_word("CI (cannot be run locally)"));
    assert!(starts_with_no_test_word("untested (the test was removed)"));
    assert!(!starts_with_no_test_word("the import stopped at the first number"));
    assert!(!starts_with_no_test_word("[ADR-0003](../decisions/0003-data-safety.md)"));
    assert!(!starts_with_no_test_word("processing"));

    let spec = "| A-001 | Rule | IS NOW | `x.rs` | `media.rs::plays_a_clip` |\n\
                | A-002 | Rule | IS NOW | `x.rs` | process |\n\
                | A-003 | Rule | IS NOW | `x.rs` | free text |\n\
                | A-004 | Rule | IS NOW | `x.rs` | `media.rs::plays_a_cliip`, `dev_998` |\n\
                | A-005 | Rule | SHOULD BE | `x.rs` | `gone.rs::nothing` |\n\
                | A-006 | Rule with | a bar | IS NOW ⚠ | `x.rs` | `gone.rs::nothing` |\n";
    assert_eq!(
        unresolved_tests(&fixture_tree(), &[("a.md", spec)]),
        [
            "A-003: no test named and the cell starts with none of [\"process\", \"CI\", \"untested\"]: free text",
            "A-004: dev_998 (no fn named or starting with dev_998 in src/ or tests/)",
            "A-004: media.rs::plays_a_cliip (no fn or mod plays_a_cliip in src/media.rs)",
            "A-006: gone.rs::nothing (file missing)",
        ]
    );
}
