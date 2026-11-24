//! The bibliography and its key index (EDIT-123, JOB-039): reading `.bib` files in the style of
//! Zotero with Better BibTeX, JabRef and Citavi (hand-written, `src/testdata/bib/`), the bad
//! entries that are skipped, the files that a note names, and the background reads.

use crate::bibliography::reader::{self, Entry, FileIndex, Problem};
use crate::bibliography::report;
use crate::bibliography::resolve;
use crate::bibliography::state::BIB_READ;
use crate::bibliography::{Bibliographies, Effect};
use crate::busy::Busy;
use crate::vault::Vault;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const BETTER_BIBTEX: &str = include_str!("testdata/bib/better-bibtex.bib");
const JABREF: &str = include_str!("testdata/bib/jabref.bib");
const CITAVI: &str = include_str!("testdata/bib/citavi.bib");

fn keys(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.key.as_str()).collect()
}

fn entry<'a>(entries: &'a [Entry], key: &str) -> &'a Entry {
    entries.iter().find(|e| e.key == key).unwrap_or_else(|| panic!("no entry {key:?} in {:?}", keys(entries)))
}

/// An empty folder of its own for a test.
fn folder(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("shale-bib-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(root: &Path, rel: &str, text: &str) {
    let f = root.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

fn index_of(src: &str) -> Arc<FileIndex> {
    let d = folder("index-of");
    write(&d, "x.bib", src);
    let index = reader::read_file(&d.join("x.bib"), None).unwrap();
    let _ = std::fs::remove_dir_all(&d);
    index
}

fn effect_of(src: &str) -> Effect {
    Effect { files: vec![(PathBuf::from("refs.bib"), index_of(src))], ..Effect::default() }
}

// spec: EDIT-123
#[test]
fn edit_123_a_better_bibtex_export_is_read_and_its_broken_entry_skipped_and_counted() {
    let p = reader::parse(BETTER_BIBTEX);
    assert_eq!(p.skipped, 1, "the thesis entry without its closing brace");
    assert_eq!(keys(&p.entries), ["knuth1984literate", "lamport1994latex", "mueller2020umlaut", "chen:2019.deep-learning", "shale2026", "after-broken"], "the entries before and after the broken one stay");
    let knuth = entry(&p.entries, "knuth1984literate");
    assert_eq!(
        (knuth.kind.as_str(), knuth.authors.as_slice(), knuth.year, knuth.title.as_str()),
        ("article", ["Knuth".to_string()].as_slice(), Some(1984), "Literate Programming"),
        "double braces are no part of the title; `date` gives the year"
    );
    assert_eq!(entry(&p.entries, "lamport1994latex").title, "LaTeX: A Document Preparation System");
    let m = entry(&p.entries, "mueller2020umlaut");
    assert_eq!(m.authors, ["Müller", "Schön"], "{{\\\"u}} and {{\\'e}} are read, `and others` is no author");
    assert!(m.et_al);
    assert_eq!((m.year, m.title.as_str()), (Some(2020), "Über die Bedeutung von ähnlichen Zeichen"));
    assert_eq!(entry(&p.entries, "shale2026").authors, ["Shale Project"], "a corporate author in braces is one name");
    assert_eq!(entry(&p.entries, "shale2026").kind, "online");
    assert_eq!(entry(&p.entries, "after-broken").year, Some(1843), "`year` without `date`");
    assert!(p.entries.iter().all(|e| e.problem.is_none()), "a key with `:`, `.` and `-` inside is a key");
}

// spec: EDIT-123
#[test]
fn edit_123_a_jabref_export_resolves_strings_crossref_and_editors() {
    let p = reader::parse(JABREF);
    assert_eq!(p.skipped, 0);
    assert_eq!(keys(&p.entries), ["Knuth1984", "Lamport1994", "Lamport1994chapter", "Handbook1999"], "`@String` and `@Comment` are no entries");
    assert_eq!(entry(&p.entries, "Knuth1984").year, Some(1984), "`year` and `month = may`");
    let chapter = entry(&p.entries, "Lamport1994chapter");
    assert_eq!(chapter.authors, ["Hopper", "Turing"]);
    assert!(chapter.et_al);
    assert_eq!(chapter.year, Some(1999), "the year comes from the entry it cross-references");
    assert_eq!(entry(&p.entries, "Handbook1999").authors, ["Gauss"], "an entry with editors only lists them");
}

// spec: EDIT-123
#[test]
fn edit_123_a_citavi_export_with_escapes_utf8_and_windows_paths() {
    let p = reader::parse(CITAVI);
    assert_eq!(p.skipped, 0);
    assert_eq!(keys(&p.entries), ["Schmidt.2018", "Kowalski.2021", "Sato.2017", "NoYear.2022"]);
    assert_eq!(entry(&p.entries, "Schmidt.2018").authors, ["Schmidt", "Weber"], "{{\\\"u}} in a given name");
    let k = entry(&p.entries, "Kowalski.2021");
    assert_eq!(k.authors, ["Kowalski", "Łukasiewicz"]);
    assert_eq!(k.title, "Straßenbau im Wandel – A review", "\\ss and -- are read");
    assert_eq!(entry(&p.entries, "Sato.2017").authors, ["佐藤", "田中"]);
    assert_eq!(entry(&p.entries, "Sato.2017").title, "日本語のタイトル");
    assert_eq!(entry(&p.entries, "NoYear.2022").year, None, "an entry without a date has no year");
}

// spec: EDIT-123
#[test]
fn edit_123_the_entries_of_a_file_with_a_bad_one_keep_their_strings_and_their_crossref() {
    let src = "@string{me = {Lovelace, Ada}}\n@book{parent, author = me, title = {The Book}, year = {1843}}\n@article{bad, title = {never closed\n@incollection{child, crossref = {parent}, author = {Doe, Jane}, title = {A Chapter}, pages = {1--2}}\n";
    let p = reader::parse(src);
    assert_eq!(p.skipped, 1);
    assert_eq!(keys(&p.entries), ["parent", "child"]);
    assert_eq!(entry(&p.entries, "parent").authors, ["Lovelace"], "the @string of the file is known to each entry");
    assert_eq!(entry(&p.entries, "child").year, Some(1843), "crossref is resolved among the entries that are left");
}

// spec: EDIT-123
#[test]
fn edit_123_a_key_that_comes_twice_is_skipped_the_second_time() {
    let p = reader::parse("@misc{dup, title = {First}, year = {2000}}\n@misc{other, title = {O}, year = {2001}}\n@misc{dup, title = {Second}, year = {2002}}\n");
    assert_eq!((p.skipped, keys(&p.entries)), (1, vec!["dup", "other"]));
    assert_eq!(entry(&p.entries, "dup").title, "First");
}

// spec: EDIT-123
#[test]
fn edit_123_the_year_comes_from_date_year_or_the_words_of_a_date() {
    let p = reader::parse("@misc{a, title = {A}, date = {2019-04-01}}\n@misc{b, title = {B}, year = {2018}}\n@misc{c, title = {C}, date = {circa 1990}}\n@misc{d, title = {D}, date = {2001/2004}}\n@misc{e, title = {E}}\n");
    let years: Vec<_> = p.entries.iter().map(|e| e.year).collect();
    assert_eq!(years, [Some(2019), Some(2018), Some(1990), Some(2001), None]);
}

// spec: EDIT-123
#[test]
fn edit_123_a_byte_order_mark_and_windows_line_endings_are_no_problem() {
    let src = format!("\u{feff}{}", JABREF.replace('\n', "\r\n"));
    let p = reader::parse(&src);
    assert_eq!((p.skipped, p.entries.len()), (0, 4));
    let src = format!("\u{feff}{}", BETTER_BIBTEX.replace('\n', "\r\n"));
    let p = reader::parse(&src);
    assert_eq!((p.skipped, p.entries.len()), (1, 6));
}

// spec: EDIT-123
#[test]
fn edit_123_a_key_that_cannot_be_cited_is_in_the_index_and_says_why() {
    let effect = effect_of("@misc{fig:one, title = {A}, year = {2000}}\n@misc{tbl:x, title = {B}, year = {2000}}\n@misc{has#hash, title = {C}, year = {2000}}\n@misc{fine, title = {D}, year = {2000}}\n");
    let index = &effect.files[0].1;
    assert_eq!(index.get("fig:one").unwrap().problem, Some(Problem::ReservedPrefix));
    assert_eq!(index.get("tbl:x").unwrap().problem, Some(Problem::ReservedPrefix));
    assert_eq!(index.get("has#hash").unwrap().problem, Some(Problem::OutsideGrammar));
    assert!(effect.has_key("fine") && !effect.has_key("fig:one") && !effect.has_key("has#hash") && !effect.has_key("missing"));
    assert_eq!(effect.entries().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["fine"]);
    let line = report::default_lines(&effect).join(" ");
    assert!(line.contains("3 keys that cannot be cited") && line.contains("starts with a prefix"), "{line}");
}

// spec: EDIT-123
#[test]
fn edit_123_a_file_that_is_missing_or_holds_nothing_to_read_is_no_bibliography() {
    let d = folder("bad-files");
    let read = |name: &str, text: &str| {
        write(&d, name, text);
        reader::read_file(&d.join(name), None).err()
    };
    assert!(reader::read_file(&d.join("nothing.bib"), None).unwrap_err().starts_with("cannot be read"));
    assert!(read("empty.bib", "").unwrap().contains("holds no entries"));
    assert!(read("prose.bib", "This is not a bibliography.\nIt has no entries.\n").unwrap().contains("holds no entries"));
    assert!(read("all-bad.bib", "@book{a, title = {open\n@book{b, title = {open\n").unwrap().contains("no entry that can be read (2 skipped)"));
    assert!(read("binary.bib", "\u{0}\u{1}\u{2}").is_some());
    assert!(read("ok.bib", "@misc{a, title = {A}, year = {1999}}").is_none());
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-123
#[test]
fn edit_123_the_index_is_in_the_order_of_the_keys_and_finds_a_key() {
    let index = index_of("@misc{b, title = {B}}\n@misc{a, title = {A}}\n@misc{c, title = {C}}\n");
    assert_eq!(keys(&index.entries), ["a", "b", "c"]);
    assert_eq!(index.get("b").map(|e| e.title.as_str()), Some("B"));
    assert!(index.get("B").is_none(), "keys are case-sensitive");
}

// spec: EDIT-123
#[test]
fn edit_123_several_files_are_one_bibliography_and_the_first_file_wins_a_key() {
    let a = index_of("@misc{x, title = {From a}, year = {2000}}\n@misc{only-a, title = {A}, year = {2000}}\n");
    let b = index_of("@misc{x, title = {From b}, year = {2001}}\n@misc{only-b, title = {B}, year = {2001}}\n");
    let effect = Effect { files: vec![(PathBuf::from("a.bib"), a), (PathBuf::from("b.bib"), b)], ..Effect::default() };
    assert!(effect.in_effect());
    assert_eq!(effect.entry("x").unwrap().title, "From a");
    assert_eq!(effect.entries().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["only-a", "x", "only-b"], "a key twice is listed once");
    assert!(effect.has_key("only-b"));
    assert!(!Effect::default().in_effect());
}

fn vault_with(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
    let d = folder(tag);
    for (rel, text) in files {
        write(&d, rel, text);
    }
    (Vault::open(&d).unwrap(), d)
}

fn names(w: &resolve::Wanted) -> Vec<String> {
    w.files.iter().map(|f| f.to_string_lossy().into_owned()).collect()
}

// spec: EDIT-123
#[test]
fn edit_123_the_property_names_the_file_as_a_logo_is_named() {
    let (vault, d) = vault_with("resolve", &[("refs.bib", ""), ("lit/more.bib", ""), ("Thesis.md", ""), ("lit/Chapter.md", "")]);
    let note = |front: &str| format!("---\n{front}\n---\ntext");
    let wanted = |front: &str, from: &str| {
        let text = note(front);
        resolve::wanted(&vault, Some((Path::new(from), &text)), "")
    };
    for front in [
        "bibliography: refs.bib",
        "bibliography: \"refs.bib\"",
        "bibliography: 'refs.bib'",
        "bibliography: [[refs.bib]]",
        "bibliography: \"[[refs.bib]]\"",
        "bibliography: ![[refs.bib|My literature]]",
        "bibliography: \"[[refs.bib|x]]\"",
        "bibliography: /refs.bib",
        "bibliography:\n  - refs.bib",
    ] {
        let w = wanted(front, "Thesis.md");
        assert_eq!((names(&w), w.problems.len()), (vec!["refs.bib".to_string()], 0), "{front}");
    }
    assert_eq!(names(&wanted("bibliography: lit/more.bib", "Thesis.md")), ["lit/more.bib"], "a path from the vault root");
    assert_eq!(names(&wanted("bibliography: more.bib", "Thesis.md")), ["lit/more.bib"], "a file name");
    assert_eq!(names(&wanted("bibliography: ./more.bib", "lit/Chapter.md")), ["lit/more.bib"], "a path from the note");
    assert_eq!(names(&wanted("bibliography: ../refs.bib", "lit/Chapter.md")), ["refs.bib"]);
    let both = wanted("bibliography: [refs.bib, lit/more.bib, refs.bib]", "Thesis.md");
    assert_eq!(names(&both), ["refs.bib", "lit/more.bib"], "a list names several, a file once");
    assert_eq!(names(&wanted("bibliography:\n  - refs.bib\n  - \"[[more.bib]]\"", "Thesis.md")), ["refs.bib", "lit/more.bib"]);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-123
#[test]
fn edit_123_without_a_property_the_vault_default_applies_and_the_property_wins() {
    let (vault, d) = vault_with("default", &[("refs.bib", ""), ("lit/more.bib", ""), ("Thesis.md", "")]);
    let plain = "no front matter";
    let own = "---\nbibliography: lit/more.bib\n---\n";
    let empty = "---\nbibliography:\ntitle: x\n---\n";
    for (text, default, expect) in [(plain, "refs.bib", "refs.bib"), (own, "refs.bib", "lit/more.bib"), (empty, "refs.bib", "refs.bib"), (plain, "[[refs.bib]]", "refs.bib"), (plain, " lit/more.bib ", "lit/more.bib")] {
        let w = resolve::wanted(&vault, Some((Path::new("Thesis.md"), text)), default);
        assert_eq!(names(&w), [expect], "{text:?} with {default:?}");
    }
    // no note (the settings page), and nothing at all
    assert_eq!(names(&resolve::wanted(&vault, None, "refs.bib")), ["refs.bib"]);
    assert_eq!(resolve::wanted(&vault, None, ""), resolve::Wanted::default());
    assert_eq!(resolve::wanted(&vault, Some((Path::new("Thesis.md"), plain)), "  "), resolve::Wanted::default());
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-123
#[test]
fn edit_123_a_name_that_is_no_file_of_the_vault_is_a_message_and_no_bibliography() {
    let (vault, d) = vault_with("missing", &[("refs.bib", "")]);
    let w = resolve::wanted(&vault, Some((Path::new("Thesis.md"), "---\nbibliography: [refs.bib, gone.bib]\n---\n")), "");
    assert_eq!(names(&w), ["refs.bib"]);
    assert_eq!(w.problems, ["The bibliography “gone.bib” is not a file of the vault."]);
    let w = resolve::wanted(&vault, None, "gone.bib");
    assert!(w.files.is_empty());
    let effect = Bibliographies::default().effect(&w);
    assert!(!effect.in_effect() && !effect.loading);
    assert_eq!(effect.messages, w.problems);
    let _ = std::fs::remove_dir_all(&d);
}

/// Tick until the reads of `bibs` are over.
fn finish(bibs: &mut Bibliographies, vault: &Vault, busy: &Busy, wanted: &[&Path]) {
    for _ in 0..1000 {
        bibs.tick(vault, busy, wanted, &|| {});
        if !bibs.running() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the bibliography is still being read");
}

// spec: EDIT-123, JOB-039
#[test]
fn job_039_a_file_is_read_in_a_task_that_the_registry_refuses_twice_and_the_result_is_the_index() {
    let (vault, d) = vault_with("task", &[("refs.bib", JABREF)]);
    let (busy, mut bibs) = (Busy::default(), Bibliographies::default());
    let rel = PathBuf::from("refs.bib");
    let wanted = resolve::wanted(&vault, None, "refs.bib");
    // while another task of the key runs, none starts (JOB-005), and the bibliography is still loading
    let held = busy.begin(BIB_READ.with("refs.bib"), "held").unwrap();
    bibs.tick(&vault, &busy, &[rel.as_path()], &|| {});
    assert!(!bibs.running());
    assert!(bibs.effect(&wanted).loading && !bibs.effect(&wanted).in_effect());
    drop(held);
    // the read is a job: it is there after the tick, and its task ends before the result is taken in
    bibs.tick(&vault, &busy, &[rel.as_path()], &|| {});
    assert!(bibs.running(), "the read runs in the background");
    finish(&mut bibs, &vault, &busy, &[rel.as_path()]);
    assert!(!busy.is_running(&BIB_READ.with("refs.bib")));
    let effect = bibs.effect(&wanted);
    assert!(effect.in_effect() && !effect.loading && effect.messages.is_empty());
    assert_eq!(effect.entries().count(), 4);
    assert!(effect.has_key("Knuth1984"));
    // nothing changed: no second read
    bibs.tick(&vault, &busy, &[rel.as_path()], &|| {});
    assert!(!bibs.running(), "an unchanged file is not read again");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-123
#[test]
fn edit_123_the_index_is_rebuilt_when_the_file_changes() {
    let (mut vault, d) = vault_with("rebuild", &[("refs.bib", "@misc{one, title = {One}, year = {2000}}\n")]);
    let (busy, mut bibs) = (Busy::default(), Bibliographies::default());
    let rel = PathBuf::from("refs.bib");
    let wanted = resolve::wanted(&vault, None, "refs.bib");
    finish(&mut bibs, &vault, &busy, &[rel.as_path()]);
    assert_eq!(bibs.effect(&wanted).entries().count(), 1);
    let first = bibs.effect(&wanted).files[0].1.modified;
    assert!(first.is_some(), "the index has the file's modified time");
    // the file gets a new entry and a later modified time
    write(&d, "refs.bib", "@misc{one, title = {One}, year = {2000}}\n@misc{two, title = {Two}, year = {2001}}\n");
    let later = SystemTime::now() + Duration::from_secs(30);
    std::fs::File::options().write(true).open(d.join("refs.bib")).unwrap().set_modified(later).unwrap();
    vault.rescan();
    bibs.tick(&vault, &busy, &[rel.as_path()], &|| {});
    assert!(bibs.running(), "the changed file is read again");
    // until the new index is there the old one stays in effect
    assert!(bibs.effect(&wanted).has_key("one"));
    finish(&mut bibs, &vault, &busy, &[rel.as_path()]);
    let effect = bibs.effect(&wanted);
    assert!(effect.has_key("two") && effect.has_key("one"));
    assert_ne!(effect.files[0].1.modified, first);
    // a file that turns bad: a message, and no bibliography from it
    write(&d, "refs.bib", "not a bibliography any more\n");
    std::fs::File::options().write(true).open(d.join("refs.bib")).unwrap().set_modified(later + Duration::from_secs(30)).unwrap();
    vault.rescan();
    finish(&mut bibs, &vault, &busy, &[rel.as_path()]);
    let effect = bibs.effect(&wanted);
    assert!(!effect.in_effect());
    assert_eq!(effect.messages.len(), 1);
    assert!(effect.messages[0].contains("“refs.bib” holds no entries") && effect.messages[0].contains("No bibliography is in effect"), "{:?}", effect.messages);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-123
#[test]
fn edit_123_the_bibliographies_of_another_vault_are_forgotten() {
    let (vault, d) = vault_with("forget-a", &[("refs.bib", JABREF)]);
    let (other, d2) = vault_with("forget-b", &[("refs.bib", CITAVI)]);
    let (busy, mut bibs) = (Busy::default(), Bibliographies::default());
    let rel = PathBuf::from("refs.bib");
    let wanted = resolve::wanted(&vault, None, "refs.bib");
    finish(&mut bibs, &vault, &busy, &[rel.as_path()]);
    assert!(bibs.effect(&wanted).has_key("Knuth1984"));
    finish(&mut bibs, &other, &busy, &[rel.as_path()]);
    let effect = bibs.effect(&wanted);
    assert!(!effect.has_key("Knuth1984") && effect.has_key("Schmidt.2018"), "the same path of another vault is another file");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&d2);
}

// spec: EDIT-123
#[test]
fn edit_123_the_note_report_counts_the_keys_it_cites_and_names_those_the_file_lacks() {
    let effect = effect_of("@misc{smith, title = {S}, year = {2000}}\n@misc{jones, title = {J}, year = {2001}}\n");
    let text = "As [@smith, p. 3] and [@jones; @nobody] show, @smith and @jones agree; [@smith] again, @todo and @nobody2.\n";
    let cited = report::cited(text, &effect);
    assert_eq!(cited.keys, ["smith", "jones", "nobody"]);
    assert_eq!(cited.unknown, ["nobody"], "a bare @todo is plain text and is not a key");
    let lines = report::note_lines("Thesis.md", text, &effect);
    assert_eq!(lines, ["Thesis.md uses refs.bib and cites 3 keys; not in the bibliography: nobody"]);
    // without a bibliography the bracketed groups are still counted, and the line says there is none
    let none = Effect::default();
    assert_eq!(report::cited(text, &none).keys, ["smith", "jones", "nobody"]);
    assert_eq!(report::note_lines("Thesis.md", text, &none), ["Thesis.md cites 3 keys and has no bibliography in effect."]);
    assert!(report::note_lines("Plain.md", "no citations here", &none).is_empty());
    // a mail's text is not cited from
    assert!(report::cited("---\ntype: email\n---\n[@smith]", &effect).keys.is_empty());
    let loading = Effect { loading: true, ..Effect::default() };
    assert_eq!(report::note_lines("Thesis.md", text, &loading), ["Reading the bibliography…"]);
}

// spec: CFG-079
#[test]
fn cfg_079_the_vault_default_is_empty_left_out_of_the_file_and_read_from_an_old_one() {
    use crate::vault_state::VaultSettings;
    let fresh = VaultSettings::default();
    assert_eq!(fresh.bibliography, "");
    let json = serde_json::to_value(&fresh).unwrap();
    assert!(json.get("bibliography").is_none(), "an empty default is not written: {json}");
    let old: VaultSettings = serde_json::from_str(r#"{"version": 1, "people_dir": "Kontakte"}"#).unwrap();
    assert_eq!((old.people_dir.as_str(), old.bibliography.as_str()), ("Kontakte", ""));
    let set: VaultSettings = serde_json::from_str(r#"{"version": 1, "bibliography": "lit/refs.bib"}"#).unwrap();
    assert_eq!(set.bibliography, "lit/refs.bib");
    let back: VaultSettings = serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
    assert_eq!(back, set);
    assert!(set.other.is_empty(), "the key is a field of its own, not an unknown one");
}

// spec: EDIT-123, JOB-039, CFG-079
#[test]
fn edit_123_the_window_reads_the_default_and_the_open_notes_files_and_rebuilds_them() {
    use crate::app::tests::test_app;
    use crate::ui_test_support::Headless;
    let (mut app, d) = test_app("bib-window");
    write(&d, "refs.bib", JABREF);
    write(&d, "other.bib", CITAVI);
    write(&d, "Plain.md", "Cites [@Knuth1984] and [@Nobody].\n");
    write(&d, "Own.md", "---\nbibliography: other.bib\n---\nCites [@Schmidt.2018] and [@Knuth1984].\n");
    app.vault.as_mut().unwrap().rescan();
    let mut hl = Headless::new();
    hl.settle(&mut app);
    assert!(!app.bib_default_effect().in_effect() && app.bib_note_effect().is_none_or(|(_, _, e)| !e.in_effect()), "nothing asks for a bibliography yet");
    // the vault default applies to a note without a property
    app.vdata.settings.bibliography = "refs.bib".into();
    app.open(Path::new("Plain.md"), true);
    app.bib_tick(hl.ctx());
    assert!(app.busy_jobs().contains(&"the bibliography reader"), "the settled window waits for the read (DEV-041)");
    hl.settle(&mut app);
    assert!(app.bib_default_effect().has_key("Knuth1984"));
    let (name, text, effect) = app.bib_note_effect().unwrap();
    assert_eq!(name, "Plain.md");
    let lines = report::note_lines(&name, &text, &effect);
    assert_eq!(lines, ["Plain.md uses refs.bib and cites 2 keys; not in the bibliography: Nobody"]);
    // a note with its own property uses that file, the default stays asked for
    app.open(Path::new("Own.md"), true);
    hl.settle(&mut app);
    let (name, text, effect) = app.bib_note_effect().unwrap();
    assert_eq!(name, "Own.md");
    assert_eq!(report::note_lines(&name, &text, &effect), ["Own.md uses other.bib and cites 2 keys; not in the bibliography: Knuth1984"]);
    assert!(app.bib_default_effect().has_key("Knuth1984"));
    // another vault forgets the files of this one
    app.bib.reset();
    assert!(!app.bib_default_effect().in_effect());
    let _ = std::fs::remove_dir_all(&d);
}
