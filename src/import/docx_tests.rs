//! Tests of the Word import (IMP-035 to IMP-039). The ones that run the real pandoc skip with a
//! printed note when it is not installed (BUILD-018); the name rules, the argument list, the search,
//! the failures and the files written are tested with stand-ins for pandoc and need no pandoc.

use super::docx;
#[cfg(unix)]
use super::docx::Convert;
use super::*;
use std::collections::HashSet;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-docx-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn temp_vault(tag: &str) -> Vault {
    Vault::open(&temp_dir(tag)).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/docx").join(name)
}

/// The pandoc of this computer, or None with a printed note: the tests that need it skip (BUILD-018).
fn real_pandoc() -> Option<PathBuf> {
    let found = docx::locate("");
    if found.is_none() {
        eprintln!("skipping: pandoc is not installed here");
    }
    found
}

fn options(keep_docx: bool) -> Options {
    Options { folder: PathBuf::from("In"), tags: vec![], keep_original: false, keep_docx, people_dir: "Adressbuch".into(), header_list: false }
}

fn month_dir() -> String {
    let (y, m, _) = media::local_date();
    format!("z_media/{y:04}/{m:02}")
}

/// Imports the fixture `name` with the real pandoc into a new vault.
fn import_fixture(tag: &str, name: &str, keep: bool) -> Option<(Vault, Imported)> {
    import_file(tag, fixture(name), keep)
}

/// Imports the Word file at `path` with the real pandoc into a new vault.
fn import_file(tag: &str, path: PathBuf, keep: bool) -> Option<(Vault, Imported)> {
    let pandoc = real_pandoc()?;
    let mut v = temp_vault(tag);
    let mut items = vec![docx::docx_item(path, Some(pandoc))];
    // ahead of time, as the dialog does it in its job
    let mut d = dialog::ImportDialog::new(std::mem::take(&mut items), vec![], "In".into(), String::new(), None, false);
    d.keep_docx = keep;
    assert!(d.start_conversion(v.root.clone()));
    let end = std::time::Instant::now() + Duration::from_secs(120);
    while !d.poll_conversion() {
        assert!(std::time::Instant::now() < end, "pandoc did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    let done = import(&mut v, &d.items, &options(keep));
    Some((v, done))
}

/// The text with the rows of its pipe tables reduced to their cells: padding and the column
/// alignment depend on the pandoc version (3.11 drops a right alignment that 3.14 keeps) and are
/// not the filter's business, the cells are.
fn loose_tables(text: &str) -> String {
    text.lines()
        .map(|l| {
            if !l.starts_with('|') {
                return l.to_string();
            }
            let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
            let delimiter = cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'));
            let cells: Vec<&str> = if delimiter { cells.iter().map(|_| "---").collect() } else { cells };
            format!("| {} |", cells.join(" | "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The note without its properties block, and the lines of that block.
fn split_props(text: &str) -> (Vec<&str>, &str) {
    let inner = text.strip_prefix("---\n").unwrap_or_else(|| panic!("no properties in {text}"));
    let (head, body) = inner.split_once("\n---\n\n").unwrap_or_else(|| panic!("no end of the properties in {text}"));
    (head.lines().collect(), body)
}

// spec: IMP-039
#[test]
fn tables_are_compared_by_their_cells_whatever_the_padding_and_alignment() {
    assert_eq!(loose_tables("a\n| A  | B |\n|:---|--:|\n| 1  |  2 |\n"), "a\n| A | B |\n| --- | --- |\n| 1 | 2 |");
    assert_eq!(loose_tables("| x |   |\n|---|---|"), "| x |  |\n| --- | --- |", "an empty cell stays a cell");
}

// spec: IMP-035, IMP-037, IMP-039
#[test]
fn a_word_file_with_text_becomes_a_note_with_the_markdown_of_the_filter() {
    let Some((v, done)) = import_fixture("text", "text.docx", false) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    assert_eq!(done.notes, vec![PathBuf::from("In/text.md")]);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let (props, body) = split_props(&text);
    assert!(props.contains(&"print-title: false") && !props.iter().any(|p| p.starts_with("source")), "the look is there, the original is not kept: {props:?}");
    let expected = "<div align=\"center\">\n\n# Sample\n\n</div>\n\n# Heading one\n\n## Bold heading\n\n\
Plain text with **bold**, *italic*, <u>under</u>, ~~gone~~, H<sub>2</sub>O, x<sup>2</sup> and `code`.\n\n\
A line\nbreak, small caps and <mark style=\"background: #FFFF00;\">marked</mark> text.\n\n\
A [web link](https://example.com), a link to the heading and a footnote.[^1]\n\n\
> A block quote over two lines.\n\n\
- first\n  - nested\n  - nested too\n- second\n\n\
1.  one\n2.  two\n\n\
| Left | Right |\n|:-----|------:|\n| a    |     1 |\n| b    |     2 |\n\n\
**Term**\n\nIts definition.\n\n\
Math inline $E = mc^{2}$ and display:\n\n$$\\int_{0}^{1}x^{2}\\, dx$$\n\n\
[^1]: The footnote text.\n";
    assert_eq!(loose_tables(body), loose_tables(expected));
    assert!(!v.root.join("z_media").exists(), "nothing but the note: no pictures, the original is not kept");
}

// spec: IMP-037, IMP-039
#[test]
fn a_table_with_merged_cells_and_a_list_in_a_cell_is_a_pipe_table() {
    let Some((v, done)) = import_fixture("tables", "tables.docx", false) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let expected = "# Tables\n\n\
| Merged across both |                                |\n\
|--------------------|--------------------------------|\n\
| Top                | Item list<br>• apple<br>• pear |\n\
| Tall               | Plain                          |\n\
|                    | Last                           |\n";
    assert_eq!(loose_tables(split_props(&text).1), loose_tables(expected), "no HTML table: the merged cell is followed by an empty one, the row span by one in the next row");
}

// spec: IMP-035, IMP-037, IMP-038, IMP-039
#[test]
fn a_picture_is_saved_in_the_import_folder_and_embedded_with_its_vault_path_and_caption() {
    let Some((v, done)) = import_fixture("picture", "picture.docx", true) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let folder = format!("{}/import_docx_picture", month_dir());
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let embed = text.lines().find(|l| l.starts_with("![[")).unwrap_or_else(|| panic!("no picture in {text}"));
    let inner = embed.trim_start_matches("![[").trim_end_matches("]]");
    assert!(inner.starts_with(&format!("{folder}/media/")) && inner.ends_with(".png"), "{embed}");
    let bytes = std::fs::read(v.root.join(inner)).expect("the embedded file is there, where the link says");
    assert_eq!(&bytes[..4], b"\x89PNG");
    assert!(text.contains("\n*A red square*\n"), "the caption follows in italics: {text}");
    assert!(text.starts_with(&format!("---\nsource: \"[[{folder}/picture.docx]]\"\n")), "{text}");
    assert!(split_props(&text).1.starts_with("# Picture\n"), "{text}");
    assert_eq!(std::fs::read(v.root.join(format!("{folder}/picture.docx"))).unwrap(), std::fs::read(fixture("picture.docx")).unwrap(), "the original is kept");
    // nothing else is in the month folder: the hidden temp folder is gone
    let names: Vec<String> = std::fs::read_dir(v.root.join(month_dir())).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["import_docx_picture"], "{names:?}");
}

// spec: IMP-035, IMP-038, IMP-039
#[test]
fn a_name_with_spaces_and_letters_beyond_ascii_gives_a_safe_folder_and_note_name() {
    let Some((v, done)) = import_fixture("unicode", "Bericht Übersicht (Entwurf) 2026.docx", true) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    assert_eq!(done.notes, vec![PathBuf::from("In/Bericht Übersicht (Entwurf) 2026.md")]);
    let folder = format!("{}/import_docx_Bericht-Übersicht-(Entwurf)-2026", month_dir());
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    assert!(text.starts_with(&format!("---\nsource: \"[[{folder}/Bericht Übersicht (Entwurf) 2026.docx]]\"\n")), "{text}");
    assert!(v.root.join(&folder).join("Bericht Übersicht (Entwurf) 2026.docx").is_file());
}

// spec: IMP-038
#[test]
fn the_folder_name_follows_the_rules_for_names() {
    let free = |_: &str| false;
    assert_eq!(docx::import_folder_name("My Report 2026", free), "import_docx_My-Report-2026");
    assert_eq!(docx::import_folder_name("  a   b  ", free), "import_docx_a-b", "runs of - become one, - is trimmed");
    assert_eq!(docx::import_folder_name("a - b -- c", free), "import_docx_a-b-c");
    assert_eq!(docx::import_folder_name("Plan #1 [final] a|b^c", free), "import_docx_Plan-1-(final)-a-b-c", "link-safe");
    assert_eq!(docx::import_folder_name("what? a:b *x*", free), "import_docx_what-a-b-x", "portable");
    assert_eq!(docx::import_folder_name("CON", free), "import_docx_CON_", "a device name gets an underscore");
    assert_eq!(docx::import_folder_name("name.", free), "import_docx_name");
    assert_eq!(docx::import_folder_name("", free), "import_docx_document");
    assert_eq!(docx::import_folder_name(" - ", free), "import_docx_document", "nothing is left");
    assert_eq!(docx::import_folder_name("???", free), "import_docx_document");
    // a taken name gets -2, -3
    let taken: HashSet<&str> = ["import_docx_a", "import_docx_a-2"].into();
    assert_eq!(docx::import_folder_name("a", |n| taken.contains(n)), "import_docx_a-3");
}

// spec: IMP-038
#[test]
fn the_whole_folder_name_is_at_most_127_bytes_also_with_a_number_and_never_cut_in_a_letter() {
    let long = "x".repeat(300);
    let name = docx::import_folder_name(&long, |_| false);
    assert_eq!(name.len(), 127);
    let umlauts = "ü".repeat(100);
    let name = docx::import_folder_name(&umlauts, |_| false);
    assert!(name.len() <= 127 && name.starts_with("import_docx_ü") && name.is_char_boundary(name.len()), "{} bytes", name.len());
    assert_eq!(name, format!("import_docx_{}", "ü".repeat(57)), "57 letters of 2 bytes after 12 bytes");
    let first = docx::import_folder_name(&long, |_| false);
    let second = docx::import_folder_name(&long, |n| n == first);
    assert_eq!(second.len(), 127, "the number is inside the limit");
    assert!(second.ends_with("-2") && second != first);
    let spaced = docx::import_folder_name(&format!("{}-", "a ".repeat(100)), |_| false);
    assert!(!spaced.ends_with('-') && spaced.len() <= 127, "{spaced}");
}

// spec: IMP-035
#[test]
fn pandoc_is_started_with_an_argument_list_for_the_file_the_filter_and_the_folder() {
    let a = docx::args(Path::new("/in/My Doc.docx"), Path::new("/tmp/t/filter.lua"), "z_media/2026/11/import_docx_My-Doc", Path::new("/tmp/t/note.md"));
    let a: Vec<String> = a.iter().map(|s| s.to_string_lossy().into_owned()).collect();
    assert_eq!(a, ["/in/My Doc.docx", "-f", "docx", "-t", "gfm", "--wrap=preserve", "--extract-media=.", "-M", "shale-media=z_media/2026/11/import_docx_My-Doc", "--lua-filter=/tmp/t/filter.lua", "-o", "/tmp/t/note.md"]);
}

// spec: IMP-035, IMP-036
#[test]
fn word_files_are_attachments_while_the_setting_is_off_items_with_pandoc_and_attachments_with_a_reason_without() {
    let src = temp_dir("items");
    let paths: Vec<PathBuf> = ["Plan.docx", "Big.DOCX", "notes.txt"].iter().map(|n| src.join(n)).collect();
    for p in &paths {
        crate::fsutil::create_new(p, b"x").unwrap();
    }
    let kinds = |word: &Word| -> Vec<(String, String)> {
        let (mut items, notes) = items_from_paths(&paths);
        assert!(notes.is_empty());
        word_items(&mut items, word);
        items.iter().map(|i| (i.kind().to_string(), i.title.clone())).collect()
    };
    let one = |k: &str, t: &str| (k.to_string(), t.to_string());
    let off = kinds(&Word::Off);
    assert_eq!(off.iter().map(|k| k.0.as_str()).collect::<Vec<_>>(), vec!["Files"], "off: all in one Files note as before");
    let on = kinds(&Word::On(Some(PathBuf::from("/usr/bin/pandoc"))));
    assert_eq!(on, vec![one("Word document", "Plan"), one("Word document", "Big"), one("Files", "notes")], "any case of the extension; the other files stay attachments");
    let missing = kinds(&Word::On(None));
    assert_eq!(missing.iter().map(|k| k.0.as_str()).collect::<Vec<_>>(), vec!["Files", "Files", "Files"]);
    let (mut items, _) = items_from_paths(&paths[..1]);
    word_items(&mut items, &Word::On(None));
    assert_eq!(items[0].warning().unwrap(), "pandoc was not found (Settings → Import). The file is saved as an attachment.");
    assert_eq!(items[0].detail().split(" · ").next().unwrap(), "Plan.docx");
    // it is saved as an attachment
    let mut v = temp_vault("missing");
    let done = import(&mut v, &items, &options(true));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    assert!(text.contains("type: files") && text.contains("- [[z_media/") && text.contains("Plan.docx]]"), "{text}");
    let _ = std::fs::remove_dir_all(&src);
}

/// A stand-in for pandoc, a shell script with this `body`, for the tests of the settings (unix only).
#[cfg(unix)]
pub(crate) fn fake_pandoc(dir: &Path, name: &str, body: &str) -> PathBuf {
    fake::script(dir, name, body)
}

/// Stand-ins for pandoc: shell scripts that follow its contract. Unix only.
#[cfg(unix)]
mod fake {
    use super::*;

    pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        crate::fsutil::write_atomic(&p, format!("#!/bin/sh\n{body}\n").as_bytes()).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// Writes one picture next to where it runs and the note to the file after `-o`.
    pub const WRITES_A_PICTURE: &str = r#"
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; --lua-filter=*) cat "${1#--lua-filter=}" > /dev/null;; -M) meta="$2"; shift;; esac; shift; done
mkdir -p media; printf 'PNG' > media/image1.png
printf '## Fake\n\n![[%s/media/image1.png]]\n' "${meta#shale-media=}" > "$out""#;

    pub fn word_file(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        crate::fsutil::create_new(&p, b"PK fake docx").unwrap();
        p
    }
}

#[cfg(unix)]
fn convert<'a>(pandoc: &'a Path, docx: &'a Path, root: &'a Path, keep: bool, timeout: Duration, cancel: &'a AtomicBool) -> Convert<'a> {
    Convert { pandoc, docx, root, date: (2026, 11, 17), keep_original: keep, timeout, cancel }
}

// spec: IMP-047, DEV-038
#[cfg(unix)]
#[test]
fn a_file_nested_too_deeply_is_refused_before_pandoc_runs_and_leaves_nothing_in_the_vault() {
    let dir = temp_dir("deep");
    let root = temp_dir("deep-vault");
    let ran = dir.join("pandoc-ran");
    let pandoc = fake::script(&dir, "pandoc", &format!("touch '{}'", ran.display()));
    let document = format!("<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{}</w:body></w:document>", "<w:sdt>".repeat(3000));
    let files: Vec<(String, Vec<u8>)> = vec![("[Content_Types].xml".into(), b"<Types/>".to_vec()), ("word/document.xml".into(), document.into_bytes())];
    let docx = dir.join("Deep.docx");
    crate::fsutil::create_new(&docx, &docx_look::zip_bytes(&files, false).unwrap()).unwrap();
    let cancel = AtomicBool::new(false);
    let e = docx::prepare(&convert(&pandoc, &docx, &root, true, docx::TIMEOUT, &cancel), &mut HashSet::new()).unwrap_err();
    assert_eq!(e, "“Deep.docx” cannot be imported: word/document.xml: the XML is nested too deeply (more than 256 levels)");
    assert!(!ran.exists(), "pandoc is not started for it");
    assert_eq!(std::fs::read_dir(&root).unwrap().flat_map(|d| walk(&d.unwrap().path())).filter(|p| p.is_file()).count(), 0, "no file in the vault");
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut all = vec![p.to_path_buf()];
    if p.is_dir() {
        for e in std::fs::read_dir(p).unwrap() {
            all.extend(walk(&e.unwrap().path()));
        }
    }
    all
}

// spec: IMP-035, IMP-038
#[cfg(unix)]
#[test]
fn pandoc_runs_in_the_folder_of_the_pictures_and_the_folder_is_renamed_into_place_on_success() {
    let dir = temp_dir("fake-ok");
    let root = temp_dir("fake-ok-vault");
    let pandoc = fake::script(&dir, "pandoc", fake::WRITES_A_PICTURE);
    let docx = fake::word_file(&dir, "My Report.docx");
    let cancel = AtomicBool::new(false);
    let mut reserved = HashSet::new();
    let p = docx::prepare(&convert(&pandoc, &docx, &root, true, docx::TIMEOUT, &cancel), &mut reserved).unwrap();
    assert_eq!(p.folder.as_deref(), Some(Path::new("z_media/2026/11/import_docx_My-Report")));
    assert_eq!(p.original.as_deref(), Some(Path::new("z_media/2026/11/import_docx_My-Report/My Report.docx")));
    assert_eq!(p.text, "## Fake\n\n![[z_media/2026/11/import_docx_My-Report/media/image1.png]]\n", "the filter gets the vault path of the final folder");
    assert!(!root.join("z_media/2026/11/import_docx_My-Report").exists(), "nothing in its place before the commit");
    p.commit(&root).unwrap();
    assert_eq!(std::fs::read(root.join("z_media/2026/11/import_docx_My-Report/media/image1.png")).unwrap(), b"PNG");
    assert_eq!(std::fs::read(root.join("z_media/2026/11/import_docx_My-Report/My Report.docx")).unwrap(), b"PK fake docx");
    let left: Vec<String> = std::fs::read_dir(root.join("z_media/2026/11")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, vec!["import_docx_My-Report"], "no hidden folder is left");
    // a second file with the same name in the same import, and one in the vault already: -2, -3
    let p2 = docx::prepare(&convert(&pandoc, &docx, &root, false, docx::TIMEOUT, &cancel), &mut reserved).unwrap();
    assert_eq!(p2.folder.as_deref(), Some(Path::new("z_media/2026/11/import_docx_My-Report-2")));
    assert_eq!(p2.original, None, "not kept");
    let p3 = docx::prepare(&convert(&pandoc, &docx, &root, false, docx::TIMEOUT, &cancel), &mut reserved).unwrap();
    assert_eq!(p3.folder.as_deref(), Some(Path::new("z_media/2026/11/import_docx_My-Report-3")), "two runs of one import do not choose the same folder");
    // a result that is let go without a commit leaves nothing
    drop((p2, p3));
    let left: Vec<String> = std::fs::read_dir(root.join("z_media/2026/11")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, vec!["import_docx_My-Report"], "{left:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

// spec: IMP-035, IMP-038
#[cfg(unix)]
#[test]
fn without_pictures_and_without_the_original_there_is_no_folder() {
    let dir = temp_dir("fake-nofolder");
    let mut v = temp_vault("fake-nofolder-vault");
    let pandoc = fake::script(&dir, "pandoc", r#"while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; esac; shift; done; printf '### Only text\n' > "$out""#);
    let docx = fake::word_file(&dir, "plain.docx");
    let mut it = docx::docx_item(docx.clone(), Some(pandoc.clone()));
    let done = import(&mut v, std::slice::from_ref(&it), &options(false));
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    assert_eq!(std::fs::read_to_string(v.abs(&done.notes[0])).unwrap(), "# Only text\n", "the lift box is ticked: ### becomes #");
    assert!(!v.root.join("z_media").exists(), "no import folder and no empty folders of the media tree: {:?}", walk_names(&v.root));
    // with the original kept there is a folder, and the note names the original and the tags
    let mut o = options(true);
    o.tags = vec!["work".into()];
    it.title = "kept".into();
    if let Source::Docx { lift, .. } = &mut it.source {
        *lift = false;
    }
    let done = import(&mut v, &[it], &o);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let folder = format!("{}/import_docx_plain", month_dir());
    assert_eq!(text, format!("---\nsource: \"[[{folder}/plain.docx]]\"\ntags: [work]\n---\n\n### Only text\n"));
    assert!(v.root.join(&folder).join("plain.docx").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: IMP-035
#[cfg(unix)]
#[test]
fn a_failure_leaves_nothing_in_the_vault_and_names_the_file_and_the_first_line_of_pandoc() {
    let dir = temp_dir("fake-fail");
    let mut v = temp_vault("fake-fail-vault");
    let pandoc = fake::script(&dir, "pandoc", "mkdir -p media; echo x > media/a.png; echo 'Error: the file is damaged' >&2; echo 'second line' >&2; exit 64");
    let docx = fake::word_file(&dir, "Broken one.docx");
    let it = docx::docx_item(docx, Some(pandoc));
    let done = import(&mut v, &[it], &options(true));
    assert!(done.notes.is_empty() && done.files.is_empty());
    assert_eq!(done.errors, vec!["pandoc could not read “Broken one.docx”: Error: the file is damaged"]);
    let all: Vec<_> = walk_names(&v.root);
    assert!(all.is_empty(), "nothing is left in the vault, not even a folder: {all:?}");
    // a program that cannot be started
    let gone = docx::docx_item(fake::word_file(&dir, "b.docx"), Some(dir.join("no-such-pandoc")));
    let done = import(&mut v, &[gone], &options(true));
    assert!(done.errors[0].starts_with("pandoc could not be started"), "{:?}", done.errors);
    assert!(walk_names(&v.root).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn walk_names(root: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            out.push(e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned());
            if e.path().is_dir() {
                stack.push(e.path());
            }
        }
    }
    out
}

// spec: IMP-035
#[cfg(unix)]
#[test]
fn a_run_that_takes_too_long_is_killed_and_so_is_one_that_is_cancelled() {
    let dir = temp_dir("fake-slow");
    let root = temp_dir("fake-slow-vault");
    let pandoc = fake::script(&dir, "pandoc", "sleep 30");
    let docx = fake::word_file(&dir, "slow.docx");
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let e = docx::prepare(&convert(&pandoc, &docx, &root, true, Duration::from_millis(300), &cancel), &mut HashSet::new()).unwrap_err();
    assert_eq!(e, "pandoc needed more than 0.3 seconds for “slow.docx” and was stopped");
    assert!(started.elapsed() < Duration::from_secs(10), "killed, not waited for");
    let stop = AtomicBool::new(true);
    let e = docx::prepare(&convert(&pandoc, &docx, &root, true, docx::TIMEOUT, &stop), &mut HashSet::new()).unwrap_err();
    assert_eq!(e, "stopped");
    assert!(!root.join("z_media").exists(), "no hidden folder is left, and no empty folders");
    assert_eq!(docx::TIMEOUT, Duration::from_secs(120), "the limit of IMP-035");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&root);
}

// spec: IMP-035, IMP-036
#[cfg(unix)]
#[test]
fn the_version_and_the_search_use_the_program_that_is_chosen() {
    let dir = temp_dir("fake-version");
    let ok = fake::script(&dir, "pandoc-ok", "echo 'pandoc 9.9.1'; echo 'Features: +lua'");
    let bad = fake::script(&dir, "pandoc-bad", "echo 'cannot start' >&2; exit 2");
    assert_eq!(docx::version(&ok).unwrap(), "pandoc 9.9.1");
    assert_eq!(docx::version(&bad).unwrap_err(), "cannot start");
    assert!(docx::version(&dir.join("none")).unwrap_err().starts_with("could not start"));
    assert_eq!(docx::locate(&ok.to_string_lossy()), Some(ok.clone()));
    assert_eq!(docx::locate(&dir.join("none").to_string_lossy()), None, "a chosen path that is no file is not replaced by another pandoc");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: IMP-035
#[cfg(unix)]
#[test]
fn a_note_that_cannot_be_written_takes_the_import_folder_out_again() {
    let dir = temp_dir("fake-nonote");
    let mut v = temp_vault("fake-nonote-vault");
    let pandoc = fake::script(&dir, "pandoc", fake::WRITES_A_PICTURE);
    let docx = fake::word_file(&dir, "a.docx");
    // the note's folder is a file: the note cannot be made
    crate::fsutil::create_new(&v.root.join("In"), b"in the way").unwrap();
    let done = import(&mut v, &[docx::docx_item(docx, Some(pandoc))], &options(true));
    assert!(done.notes.is_empty() && done.errors.len() == 1 && done.errors[0].starts_with("could not write the note for “a.docx”"), "{:?}", done.errors);
    assert!(!v.root.join(format!("{}/import_docx_a", month_dir())).exists(), "the folder was taken out again");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: IMP-040, IMP-041, IMP-042, IMP-044, IMP-046, IMP-039
#[test]
fn a_styled_word_file_keeps_its_page_its_body_its_colours_and_its_title() {
    let Some((v, done)) = import_fixture("styled", "styled.docx", true) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let folder = format!("{}/import_docx_styled", month_dir());
    let expected_props = format!(
        "---\nsource: \"[[{folder}/styled.docx]]\"\nprint-paper: Letter\nprint-margins: 20 25 20 30\nprint-font: serif\nprint-font-size: 12\nprint-line-spacing: 1.15\n\
print-paragraph-spacing: 8\nprint-align: justify\nprint-heading-color: \"#2F5496\"\nprint-heading-weight: bold\nprint-heading-sizes: \"20 16 14 12 12 12\"\nprint-break-before: 1\nprint-title: false\nprint-toc: false\nnumbered-headings: false\n---\n\n"
    );
    assert!(text.starts_with(&expected_props), "{text}");
    let body = &text[expected_props.len()..];
    // IMP-042: the title and the subtitle of Word's styles
    assert!(body.starts_with("<div align=\"center\">\n\n# Styled report\n\n*A look to keep*\n\n</div>\n\n# Heading one\n"), "Word centres Title and Subtitle (IMP-046): {body}");
    // IMP-041
    assert!(body.contains("<span style=\"color: #C00000;\">red text</span>"), "{body}");
    assert!(body.contains("<mark style=\"background: #00FF00;\">green highlight</mark>"), "{body}");
    assert!(body.contains("a <mark style=\"background: #FFFF00;\">yellow highlight</mark>"), "yellow is Word's FFFF00 too: {body}");
    assert!(!body.contains("=="), "no pandoc mark is left: {body}");
    assert!(body.contains("**<span style=\"color: #0000FF;\">bold blue</span>** and on"), "white space stays outside, so the emphasis works: {body}");
    assert!(body.contains("<span style=\"color: #00B050;\"><mark style=\"background: #FFC000;\">green on orange</mark></span>"), "{body}");
    assert!(body.contains("<span style=\"color: #7030A0;\">purple by style</span> and on"), "a colour from a character style: {body}");
    assert!(body.contains("<span style=\"color: #0F4761;\">shaded accent</span> and <span style=\"color: #E97132;\">plain accent</span>"), "theme colours with and without a shade: {body}");
    // IMP-044: Heading 1 starts a page by its style (the property), so no line before it; the rest are lines
    let div = "<div style=\"page-break-after: always;\"></div>";
    assert!(body.contains(&format!("Before a manual break\n\n{div}\n\nafter it.\n\n{div}\n\nStarts a page by itself.\n\nEnds a section.\n\n{div}\n\nEnds a continuous section.\n\nThe last paragraph.")), "{body}");
    assert_eq!(body.matches(div).count(), 3, "{body}");
    assert!(!body.chars().any(|c| ('\u{E000}'..='\u{E004}').contains(&c)), "no sentinel is left");
    // the original is kept as it is, without the tags
    let kept = std::fs::read(v.root.join(format!("{folder}/styled.docx"))).unwrap();
    assert_eq!(kept, std::fs::read(fixture("styled.docx")).unwrap());
}

/// A small Word file in `dir`: a justified body (styles), and `paragraphs` as the XML of the body.
fn aligned_word_file(dir: &Path, paragraphs: &str) -> PathBuf {
    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let parts = dir.join("parts");
    let styles = format!(
        r#"<w:styles {W}><w:docDefaults><w:pPrDefault><w:pPr><w:jc w:val="both"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Centred"><w:name w:val="Centred"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr></w:style></w:styles>"#
    );
    let parts_xml = [
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.to_string()),
        ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_string()),
        ("word/_rels/document.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_string()),
        ("word/styles.xml", styles),
        ("word/document.xml", format!("<w:document {W}><w:body>{paragraphs}</w:body></w:document>")),
    ];
    for (name, text) in parts_xml {
        std::fs::create_dir_all(parts.join(name).parent().unwrap()).unwrap();
        std::fs::write(parts.join(name), text).unwrap();
    }
    let docx = dir.join("aligned.docx");
    super::docx_look::write_zip(&parts, &docx).unwrap();
    docx
}

// spec: IMP-046, IMP-039
#[test]
fn centred_right_and_left_paragraphs_of_a_word_file_become_wrappers() {
    let dir = temp_dir("aligned-src");
    let p = |ppr: &str, text: &str| format!("<w:p><w:pPr>{ppr}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>");
    let body = [
        p(r#"<w:pStyle w:val="Heading1"/><w:jc w:val="center"/>"#, "Centred heading"),
        p(r#"<w:pStyle w:val="Centred"/>"#, "Centred by style"),
        p(r#"<w:pStyle w:val="Centred"/>"#, "Second centred line"),
        p("", "Justified like the body."),
        p(r#"<w:jc w:val="right"/>"#, "Flush right"),
        p(r#"<w:jc w:val="left"/>"#, "Left in a justified body"),
        p(r#"<w:pStyle w:val="Heading1"/>"#, "A plain heading"),
    ]
    .concat();
    let docx = aligned_word_file(&dir, &body);
    let Some((v, done)) = import_file("aligned", docx, false) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let (props, body) = split_props(&text);
    assert!(props.contains(&"print-align: justify"), "{props:?}");
    let c = "<div align=\"center\">";
    let expected = format!("{c}\n\n# Centred heading\n\nCentred by style\n\nSecond centred line\n\n</div>\n\nJustified like the body.\n\n<div align=\"right\">\n\nFlush right\n\n</div>\n\n<div align=\"left\">\n\nLeft in a justified body\n\n</div>\n\n# A plain heading\n");
    assert_eq!(body.trim_start(), expected, "{body}");
    assert!(!body.chars().any(|c| ('\u{E000}'..='\u{E006}').contains(&c)), "no sentinel is left");
    assert_eq!(crate::markdown::align_blocks(body).len(), 3);
    assert_eq!(crate::markdown::find_headings(body).len(), 2, "the headings are headings, wrapped or not");
}

// spec: IMP-040, IMP-042, IMP-039
#[test]
fn a_file_made_by_pandoc_alone_has_its_properties_and_a_heading_for_its_title() {
    let Some((v, done)) = import_fixture("plainprops", "text.docx", false) else { return };
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let (props, body) = split_props(&text);
    assert!(!props.iter().any(|p| p.starts_with("source")), "the block is there although the original is not kept: {props:?}");
    assert_eq!(props[props.len() - 3..], ["print-title: false", "print-toc: false", "numbered-headings: false"], "{props:?}");
    // the body font is the theme font Aptos (minorHAnsi in the theme of the file), which is sans (#366)
    assert!(props.contains(&"print-font: sans"), "{props:?}");
    assert!(body.starts_with("<div align=\"center\">\n\n# Sample\n\n</div>\n\n# Heading one\n"), "the title of the file is a heading, centred as Word has it (IMP-046): {body}");
}

/// A Word table of contents is not imported as text and an ordinary list of links stays.
// spec: IMP-043, IMP-037, IMP-039
#[test]
fn a_word_table_of_contents_is_dropped_with_its_title_and_headings_stay_but_other_links_stay() {
    let Some((v, done)) = import_fixture("toc", "toc.docx", false) else { return };
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let (props, body) = split_props(&text);
    assert!(!props.iter().any(|p| p.starts_with("print-toc")), "the export makes its own contents: {props:?}");
    assert!(!body.contains("Inhaltsverzeichnis"), "no title of the contents: {body}");
    assert!(!body.contains("Einleitung 2") && !body.contains("Details 2"), "no entries: {body}");
    assert_eq!(body, "# Einleitung\n\nErster Absatz.\n\n# Details\n\nZweiter Absatz.\n");

    // one link alone, a link in a list: not a contents, so they are kept (as their text)
    let Some((v, done)) = import_fixture("toclinks", "toclinks.docx", false) else { return };
    let text = std::fs::read_to_string(v.abs(&done.notes[0])).unwrap();
    let body = split_props(&text).1;
    assert!(body.contains("\nAlpha\n\nText between the two links.\n\nBeta\n"), "{body}");
    assert!(body.contains("- Alpha\n- Beta\n"), "{body}");
}

/// A real (small) Word file with one red run, as a path in `dir`.
#[cfg(unix)]
fn red_word_file(dir: &Path) -> PathBuf {
    let parts = dir.join("parts");
    let document = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:color w:val="C00000"/></w:rPr><w:t>red</w:t></w:r></w:p></w:body></w:document>"#;
    for (name, text) in [("[Content_Types].xml", "<Types/>"), ("word/document.xml", document)] {
        std::fs::create_dir_all(parts.join(name).parent().unwrap()).unwrap();
        std::fs::write(parts.join(name), text).unwrap();
    }
    let docx = dir.join("red.docx");
    super::docx_look::write_zip(&parts, &docx).unwrap();
    docx
}

// spec: IMP-041
#[cfg(unix)]
#[test]
fn pandoc_gets_the_tagged_copy_and_when_it_cannot_read_that_the_original() {
    let dir = temp_dir("fake-tagged");
    let root = temp_dir("fake-tagged-vault");
    let docx = red_word_file(&dir);
    let cancel = AtomicBool::new(false);
    // the note says which file pandoc was given
    let names = r#"while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; -f|-t|-M) shift;; -*) ;; *) input="$1";; esac; shift; done"#;
    let says = fake::script(&dir, "pandoc-says", &format!("{names}\nprintf '%s\\n' \"$(basename \"$input\")\" > \"$out\""));
    let p = docx::prepare(&convert(&says, &docx, &root, false, docx::TIMEOUT, &cancel), &mut HashSet::new()).unwrap();
    assert_eq!(p.text, "tagged.docx\n", "the copy with the colours tagged");
    // it cannot read the copy: the original is tried, and nothing of the copy is left
    let picky = fake::script(&dir, "pandoc-picky", &format!("{names}\ncase \"$input\" in *tagged.docx) echo 'Error: the copy is damaged' >&2; exit 64;; esac\nprintf '%s\\n' \"$(basename \"$input\")\" > \"$out\""));
    let p = docx::prepare(&convert(&picky, &docx, &root, false, docx::TIMEOUT, &cancel), &mut HashSet::new()).unwrap();
    assert_eq!(p.text, "red.docx\n");
    // and when the original fails too, its error is the one reported
    let broken = fake::script(&dir, "pandoc-broken", "echo 'Error: cannot read it' >&2; exit 64");
    let e = docx::prepare(&convert(&broken, &docx, &root, false, docx::TIMEOUT, &cancel), &mut HashSet::new()).unwrap_err();
    assert_eq!(e, "pandoc could not read “red.docx”: Error: cannot read it");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&root);
}
