//! Tests of the Word export (DOCX-001 to DOCX-010). The ones that run the real pandoc skip with a printed
//! note when it is not installed (BUILD-018); the text preparation, the styles and the finishing are tested
//! on small parts and need none.

use super::package::Package;
use super::schema_check::{self, Outcome};
use super::*;
use crate::import::docx_look;
use crate::import::{docx, Options};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-docxexp-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/docx").join(name)
}

/// The export of the real pandoc, checked against the WordprocessingML schema (DOCX-010): the test fails when Word's own
/// format would not accept the file (the check is skipped with a printed note without xmllint or the schemas).
fn export_valid(job: &Job) -> Vec<u8> {
    let bytes = export(job, &AtomicBool::new(false)).unwrap();
    schema_check::assert_valid(&bytes, "the export");
    bytes
}

/// The pandoc of this computer, or None with a printed note: the tests that need it skip (BUILD-018).
fn real_pandoc() -> Option<PathBuf> {
    let found = docx::locate("");
    if found.is_none() {
        eprintln!("skipping: pandoc is not installed here");
    }
    found
}

/// The words of a Word file as pandoc reads it as plain text: no token without a letter or a digit, no punctuation at the ends.
fn words_of(pandoc: &Path, file: &Path) -> Vec<String> {
    let out = Command::new(pandoc).arg(file).args(["-t", "plain", "--wrap=none"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).split_whitespace().map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).filter(|t| !t.is_empty()).collect()
}

/// The share of the words of `original` that `exported` has again (each word as often as it is there).
fn share(original: &[String], exported: &[String]) -> f64 {
    let mut have: HashMap<&str, usize> = HashMap::new();
    for w in exported {
        *have.entry(w).or_default() += 1;
    }
    let mut found = 0;
    for w in original {
        if let Some(n) = have.get_mut(w.as_str()).filter(|n| **n > 0) {
            *n -= 1;
            found += 1;
        }
    }
    found as f64 / original.len().max(1) as f64
}

/// A vault in a temp folder.
fn temp_vault(tag: &str) -> Vault {
    Vault::open(&temp_dir(tag)).unwrap()
}

/// The Word file imported into a new vault (the original kept), and the Word file exported from the note.
struct Trip {
    /// the note's text
    text: String,
    /// the exported file
    exported: PathBuf,
    bytes: Vec<u8>,
}

/// Import `file` into a new vault with the real pandoc (the original kept), export the note; None without pandoc, an
/// error naming the step ("import: …", "export: …") when one fails.
fn try_round_trip(tag: &str, file: &Path) -> Option<Result<Trip, String>> {
    let pandoc = real_pandoc()?;
    let mut vault = temp_vault(tag);
    let item = docx::docx_item(file.to_path_buf(), Some(pandoc.clone()));
    let options = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false, cut: Default::default() };
    let note = match docx::import_item(&mut vault, &item, &options) {
        Ok(note) => note,
        Err(e) => return Some(Err(format!("import: {e}"))),
    };
    vault.rescan();
    let text = std::fs::read_to_string(vault.abs(&note)).unwrap();
    let original = original_of(&vault, &note, &text);
    let job = Job::new(pandoc, &vault, &note, &text, original.as_deref(), false, true);
    let bytes = match export(&job, &AtomicBool::new(false)) {
        Ok(bytes) => bytes,
        Err(e) => return Some(Err(format!("export: {e}"))),
    };
    let exported = vault.root.join("out.docx");
    std::fs::write(&exported, &bytes).unwrap();
    Some(Ok(Trip { text, exported, bytes }))
}

/// The same for a file that has to go through: None without pandoc.
fn round_trip(tag: &str, file: &Path) -> Option<Trip> {
    try_round_trip(tag, file).map(|trip| trip.unwrap())
}

fn part(bytes: &[u8], name: &str) -> String {
    Package::read(bytes).unwrap().text(name).unwrap().to_string()
}

// ---------------------------------------------------------------- DOCX-003: the text pandoc reads

/// What a vault with `z_media/a b.png` and `Notes/Other.md` resolves.
fn resolver(target: &str) -> Option<String> {
    match target {
        "a b.png" | "z_media/a b.png" => Some("z_media/a b.png".to_string()),
        "Other" => Some("Notes/Other.md".to_string()),
        "odd" => Some("z_media/odd#1.png".to_string()),
        _ => None,
    }
}

// spec: DOCX-003
#[test]
fn the_front_matter_is_taken_off_and_a_note_without_it_stays() {
    assert_eq!(prepare_text("---\ntitle: x\n---\nText\n", &resolver), "Text\n");
    assert_eq!(prepare_text("---\r\nprint-font: serif\r\n---\r\nText\r\n", &resolver), "Text\r\n");
    assert_eq!(prepare_text("Text\n\n---\n\nmore\n", &resolver), "Text\n\n---\n\nmore\n", "a rule in the text is no front matter");
    assert_eq!(prepare_text("", &resolver), "");
}

// spec: DOCX-003
#[test]
fn an_embed_that_resolves_has_the_vault_path_of_its_file_with_what_follows_the_target() {
    assert_eq!(prepare_text("![[a b.png]]\n", &resolver), "![[z_media/a b.png]]\n");
    assert_eq!(prepare_text("x ![[a b.png|300]] y ![[Other#Part|Words]] z\n", &resolver), "x ![[z_media/a b.png|300]] y ![[Notes/Other.md#Part|Words]] z\n");
    assert_eq!(prepare_text("| ![[a b.png\\|300]] |\n", &resolver), "| ![[z_media/a b.png\\|300]] |\n", "in a table cell the bar is escaped");
    assert_eq!(prepare_text("![[ü ä.png]] ![[a b.png]] é\n", &resolver), "![[ü ä.png]] ![[z_media/a b.png]] é\n", "letters beyond ASCII stay");
    assert_eq!(prepare_text("![[missing.png|20]]\n", &resolver), "![[missing.png|20]]\n", "an embed that does not resolve stays");
    assert_eq!(prepare_text("![[odd]]\n", &resolver), "![[odd]]\n", "a path that the link could not hold stays");
    assert_eq!(prepare_text("a [[Other]] link and ![[Other\n", &resolver), "a [[Other]] link and ![[Other\n", "a link is no embed; an embed without its end stays");
}

// spec: DOCX-018
#[test]
fn docx_018_a_carousel_is_every_image_in_order_each_alone_with_its_caption() {
    let text = "> [!carousel] Layers\n> ![[a b.png|First|300]]\n> ![[a b.png|Second]]\n";
    assert_eq!(prepare_text(text, &resolver), "**Layers**\n\n![[z_media/a b.png|300]]\n\n*First*\n\n![[z_media/a b.png]]\n\n*Second*\n");
    assert_eq!(prepare_text("> [!note] Layers\n> ![[a b.png]]\n", &resolver), "> [!note] Layers\n> ![[z_media/a b.png]]\n", "any other quote stays");
}

// spec: DOCX-003
#[test]
fn nothing_in_code_is_changed() {
    let code = "```\n![[a b.png]]\n```\n~~~sh\n![[a b.png]]\n~~~\n````\n```\n![[a b.png]]\n````\n";
    assert_eq!(prepare_text(code, &resolver), code, "fenced code, also a short fence inside a long one");
    assert_eq!(prepare_text("`![[a b.png]]` and ``a ` ![[a b.png]]``\n", &resolver), "`![[a b.png]]` and ``a ` ![[a b.png]]``\n", "inline code");
    assert_eq!(prepare_text("`a ![[a b.png]]\n", &resolver), "`a ![[z_media/a b.png]]\n", "a backtick without its end is a letter");
    assert_eq!(prepare_text("``a ` ![[a b.png]]\n", &resolver), "``a ` ![[z_media/a b.png]]\n", "a shorter run does not end the code");
    assert_eq!(prepare_text("```js ![[a b.png]]``` ![[a b.png]]\n", &resolver), "```js ![[a b.png]]``` ![[z_media/a b.png]]\n", "three backticks and text in one line are code, not a fence");
    assert_eq!(prepare_text("```\nopen\n![[a b.png]]\n", &resolver), "```\nopen\n![[a b.png]]\n", "a fence that is not closed runs to the end");
    assert_eq!(prepare_text("```\n![[a b.png]]\n```\n![[a b.png]]\n", &resolver), "```\n![[a b.png]]\n```\n![[z_media/a b.png]]\n", "after the fence it is text again");
    assert_eq!(prepare_text("   ```\nx\n   ```\n![[a b.png]]\n", &resolver), "   ```\nx\n   ```\n![[z_media/a b.png]]\n", "a fence may be indented");
}

// ---------------------------------------------------------------- DOCX-002: the arguments

// spec: DOCX-002
#[test]
fn pandoc_is_started_with_an_argument_list_for_the_note_the_filter_the_folder_and_the_reference() {
    let say = |toc| -> Vec<String> {
        args(Path::new("/t/note.md"), Path::new("/t/filter.lua"), Path::new("/v/My Vault"), Path::new("/t/reference.docx"), 6.5, Path::new("/t/out.docx"), toc).iter().map(|a| a.to_string_lossy().into_owned()).collect()
    };
    let base = [
        "/t/note.md",
        "-f",
        "commonmark+pipe_tables+strikeout+footnotes+tex_math_dollars+wikilinks_title_after_pipe+autolink_bare_uris+task_lists",
        "-t",
        "docx",
        "--lua-filter=/t/filter.lua",
        "--resource-path=/v/My Vault",
        "--reference-doc=/t/reference.docx",
    ];
    assert_eq!(say(false), [&base[..], &["-M", "shale-text-width=6.500", "-o", "/t/out.docx"]].concat());
    assert_eq!(say(true), [&base[..], &["--toc", "--toc-depth=3", "-M", "shale-text-width=6.500", "-o", "/t/out.docx"]].concat());
}

// ---------------------------------------------------------------- DOCX-001, DOCX-007: the original

// spec: DOCX-001
#[test]
fn the_original_is_the_word_file_the_source_property_links_to() {
    let dir = temp_dir("original");
    for (f, c) in [("Imports/Old.docx", "x"), ("Imports/Sheet.xlsx", "x"), ("Notes/Report.md", "x")] {
        std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
        std::fs::write(dir.join(f), c).unwrap();
    }
    let vault = Vault::open(&dir).unwrap();
    let rel = Path::new("Notes/Report.md");
    let of = |source: &str| original_of(&vault, rel, &format!("---\nsource: {source}\n---\nText\n"));
    // spec: DOCX-001
    assert_eq!(of("\"[[Imports/Old.docx]]\""), Some(PathBuf::from("Imports/Old.docx")));
    assert_eq!(of("\"[[Old.docx|The original]]\""), Some(PathBuf::from("Imports/Old.docx")), "by name, with words");
    assert_eq!(of("\"[[Imports/Old.docx#x]]\""), Some(PathBuf::from("Imports/Old.docx")), "a heading after the name is not part of it");
    assert_eq!(of("\"[[Imports/Gone.docx]]\""), None, "it is not there any more");
    assert_eq!(of("\"[[Imports/Sheet.xlsx]]\""), None, "not a Word file");
    assert_eq!(of("\"https://example.com/a.docx\""), None, "not a link of the vault");
    assert_eq!(of("\"\""), None);
    assert_eq!(original_of(&vault, rel, "No properties\n"), None);
    assert_eq!(original_of(&vault, rel, "---\ntitle: x\n---\n"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- the zip and the parts

/// The central directory of a zip: the name and the method of each file.
fn names(bytes: &[u8]) -> Vec<(String, u16)> {
    let eocd = bytes.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06]).unwrap();
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
    let mut at = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let mut out = vec![];
    for _ in 0..u16_at(eocd + 10) {
        let (n, m, k) = (u16_at(at + 28), u16_at(at + 30), u16_at(at + 32));
        out.push((String::from_utf8(bytes[at + 46..at + 46 + n].to_vec()).unwrap(), u16_at(at + 10) as u16));
        at += 46 + n + m + k;
    }
    out
}

fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let files: Vec<(String, Vec<u8>)> = parts.iter().map(|(n, c)| (n.to_string(), c.as_bytes().to_vec())).collect();
    docx_look::zip_bytes(&files, true).unwrap()
}

// spec: DOCX-009
#[test]
fn the_zip_is_compressed_where_that_helps_and_reads_back_with_the_same_parts() {
    let text = "<w:p>The same words again and again. </w:p>".repeat(200);
    // bytes that nothing can make smaller (a picture)
    let mut seed = 12345u32;
    let noise: Vec<u8> = (0..400)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        })
        .collect();
    let files: Vec<(String, Vec<u8>)> = vec![("[Content_Types].xml".into(), text.clone().into_bytes()), ("word/media/a.png".into(), noise), ("tiny.txt".into(), vec![])];
    let packed = docx_look::zip_bytes(&files, true).unwrap();
    let stored = docx_look::zip_bytes(&files, false).unwrap();
    assert!(packed.len() < stored.len() / 4, "{} against {}", packed.len(), stored.len());
    assert_eq!(names(&packed), [("[Content_Types].xml".to_string(), 8), ("word/media/a.png".to_string(), 0), ("tiny.txt".to_string(), 0)], "deflated, and stored when that is not smaller");
    assert!(names(&stored).iter().all(|(_, m)| *m == 0));
    for z in [&packed, &stored] {
        let back = crate::pdf::tex::archive::read_zip_files(z).unwrap();
        assert_eq!(back, files, "the same names, the same bytes");
    }
    assert_eq!(docx_look::zip_bytes(&files, true).unwrap(), packed, "the same bytes for the same files");
    let many: Vec<(String, Vec<u8>)> = (0..=u16::MAX as usize).map(|i| (format!("f{i}"), vec![])).collect();
    assert_eq!(docx_look::zip_bytes(&many, false).unwrap_err(), "too many files");
}

// spec: DOCX-009
#[test]
fn a_package_is_read_changed_and_packed_with_the_content_types_first() {
    let z = zip_of(&[("word/document.xml", "<d/>"), ("[Content_Types].xml", "<t/>"), ("word/media/x.png", "PNG")]);
    let mut p = Package::read(&z).unwrap();
    assert_eq!(p.text("word/document.xml").unwrap(), "<d/>");
    assert!(p.text("word/none.xml").unwrap_err().contains("no word/none.xml"));
    p.set("word/document.xml", "<e/>".to_string());
    p.set("word/none.xml", "ignored".to_string());
    p.retain(|n| n != "word/media/x.png");
    assert_eq!(p.names().collect::<Vec<_>>(), ["word/document.xml", "[Content_Types].xml"]);
    let back = p.pack().unwrap();
    assert_eq!(names(&back).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["[Content_Types].xml", "word/document.xml"]);
    assert_eq!(Package::read(&back).unwrap().text("word/document.xml").unwrap(), "<e/>");
    assert!(Package::read(b"not a zip").is_err());
    assert!(Package::read(&zip_of(&[("a.txt", "x")])).unwrap_err().contains("no Word file"));
    // a part that is not UTF-8 text
    let files = vec![("[Content_Types].xml".to_string(), b"<t/>".to_vec()), ("word/document.xml".to_string(), vec![0xff, 0xfe])];
    let odd = docx_look::zip_bytes(&files, false).unwrap();
    assert!(Package::read(&odd).unwrap().text("word/document.xml").unwrap_err().contains("not UTF-8"));
}

// ---------------------------------------------------------------- DOCX-007: the original as the template

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn styles_xml(body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<w:styles xmlns:w=\"{W}\">{body}</w:styles>")
}

/// A few styles as pandoc's default reference document has them.
fn pandoc_styles() -> String {
    let heading = |n: u32, pt: u32, extra_p: &str, fonts: &str| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\" /><w:basedOn w:val=\"Normal\" /><w:next w:val=\"BodyText\" /><w:link w:val=\"Heading{n}Char\" /><w:qFormat />\
             <w:pPr><w:keepNext /><w:keepLines />{extra_p}<w:spacing w:before=\"160\" w:after=\"80\" /><w:outlineLvl w:val=\"{}\" /></w:pPr>\
             <w:rPr>{fonts}<w:color w:val=\"0F4761\" w:themeColor=\"accent1\" w:themeShade=\"BF\" /><w:sz w:val=\"{pt}\" /><w:szCs w:val=\"{pt}\" /></w:rPr></w:style>",
            n - 1
        )
    };
    let major = "<w:rFonts w:asciiTheme=\"majorHAnsi\" w:eastAsiaTheme=\"majorEastAsia\" w:hAnsiTheme=\"majorHAnsi\" w:cstheme=\"majorBidi\" />";
    let east = "<w:rFonts w:eastAsiaTheme=\"majorEastAsia\" w:cstheme=\"majorBidi\" />";
    let mut body = String::from(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme=\"minorHAnsi\" w:eastAsiaTheme=\"minorEastAsia\" w:hAnsiTheme=\"minorHAnsi\" w:cstheme=\"minorBidi\" /><w:sz w:val=\"24\" /><w:szCs w:val=\"24\" /></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"200\" /></w:pPr></w:pPrDefault></w:docDefaults>\n\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\" /><w:qFormat /></w:style>\n\
         <w:style w:type=\"paragraph\" w:styleId=\"BodyText\"><w:name w:val=\"Body Text\" /><w:basedOn w:val=\"Normal\" /><w:link w:val=\"BodyTextChar\" /><w:qFormat /><w:pPr><w:spacing w:before=\"180\" w:after=\"180\" /></w:pPr></w:style>\n\
         <w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"FirstParagraph\"><w:name w:val=\"First Paragraph\" /><w:basedOn w:val=\"BodyText\" /><w:next w:val=\"BodyText\" /><w:qFormat /></w:style>\n\
         <w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\" /><w:basedOn w:val=\"Normal\" /><w:next w:val=\"BodyText\" /><w:qFormat /><w:pPr><w:jc w:val=\"center\" /></w:pPr><w:rPr>",
    );
    body.push_str(major);
    body.push_str("<w:sz w:val=\"56\" /><w:szCs w:val=\"56\" /></w:rPr></w:style>\n");
    for n in 1..=6 {
        body.push_str(&heading(n, [40, 32, 28, 0, 0, 0][n as usize - 1].max(24), "", if n < 3 { major } else { east }));
        body.push('\n');
    }
    body.push_str(
        "<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\" /></w:style>\n\
         <w:style w:type=\"table\" w:default=\"1\" w:styleId=\"Table\"><w:name w:val=\"Table\" /><w:basedOn w:val=\"TableNormal\" /><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\" /></w:tblPr></w:style>\n\
         <w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"BodyTextChar\"><w:name w:val=\"Body Text Char\" /><w:basedOn w:val=\"DefaultParagraphFont\" /><w:link w:val=\"BodyText\" /></w:style>\n\
         <w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"VerbatimChar\"><w:name w:val=\"Verbatim Char\" /><w:basedOn w:val=\"BodyTextChar\" /><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" /><w:sz w:val=\"22\" /></w:rPr></w:style>\n",
    );
    styles_xml(&body)
}

fn style_ids(xml: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    doc.root_element().children().filter(|n| n.is_element() && n.tag_name().name() == "style").map(|n| n.attribute((W, "styleId")).unwrap().to_string()).collect()
}

/// The element of the style `id` as text.
fn style_text<'a>(xml: &'a str, id: &str) -> &'a str {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let n = doc.root_element().children().find(|n| n.is_element() && n.attribute((W, "styleId")) == Some(id)).unwrap_or_else(|| panic!("no style {id} in {xml}"));
    &xml[n.range()]
}

// spec: DOCX-007
#[test]
fn the_styles_of_pandoc_are_merged_into_the_originals_without_double_ones() {
    let original = styles_xml(
        "<w:docDefaults/>\n<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>\n\
         <w:style w:type=\"paragraph\" w:styleId=\"heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/></w:style>\n\
         <w:style w:type=\"paragraph\" w:styleId=\"TITLE\"><w:name w:val=\"Titel\"/></w:style>\n\
         <w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/></w:style>\n\
         <w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/></w:style>",
    );
    let merged = reference::merge_styles(&original, &pandoc_styles()).unwrap();
    // the original stays as it was, the missing ones follow it
    assert!(merged.starts_with(original.trim_end_matches("</w:styles>")), "the text of the original is kept");
    let ids = style_ids(&merged);
    assert_eq!(&ids[..5], ["Normal", "heading1", "TITLE", "DefaultParagraphFont", "TableNormal"]);
    assert_eq!(
        &ids[5..],
        ["BodyText", "FirstParagraph", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6", "Table", "BodyTextChar", "VerbatimChar"],
        "not Normal, Heading 1 (by name, case ignored), Title (by id, case ignored) or Default Paragraph Font; Heading 1's own are not doubled"
    );
    // Body Text without its spacing, the default of the table style dropped (the original has its own)
    let body = style_text(&merged, "BodyText");
    assert!(!body.contains("spacing") && !body.contains("pPr") && body.contains("<w:basedOn w:val=\"Normal\" />"), "{body}");
    assert!(!style_text(&merged, "Table").contains("w:default") && style_text(&merged, "Table").contains("w:styleId=\"Table\""));
    assert!(style_text(&merged, "Heading2").contains("<w:basedOn w:val=\"Normal\" />") && style_text(&merged, "Heading2").contains("<w:spacing"), "other styles keep their spacing");
    roxmltree::Document::parse(&merged).unwrap();
}

// spec: DOCX-007
#[test]
fn a_style_that_names_another_of_pandoc_is_renamed_to_the_id_the_original_has_for_that_name() {
    // a German Word file: Normal is `Standard`, Body Text is `Textkrper`, a link style has another id
    let original = styles_xml(
        "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/></w:style>\n\
         <w:style w:type=\"paragraph\" w:styleId=\"Textkrper\"><w:name w:val=\"BODY text\"/><w:basedOn w:val=\"Standard\"/></w:style>\n\
         <w:style w:type=\"character\" w:styleId=\"TextkrperZchn\"><w:name w:val=\"Body Text Char\"/></w:style>",
    );
    let merged = reference::merge_styles(&original, &pandoc_styles()).unwrap();
    let ids = style_ids(&merged);
    assert!(!ids.contains(&"BodyText".to_string()) && !ids.contains(&"BodyTextChar".to_string()) && !ids.contains(&"Normal".to_string()), "{ids:?}");
    let first = style_text(&merged, "FirstParagraph");
    assert!(first.contains("<w:basedOn w:val=\"Textkrper\"/>") && first.contains("<w:next w:val=\"Textkrper\"/>"), "{first}");
    let h = style_text(&merged, "Heading2");
    assert!(h.contains("<w:basedOn w:val=\"Standard\"/>") && h.contains("<w:next w:val=\"Textkrper\"/>") && h.contains("<w:link w:val=\"Heading2Char\" />"), "a link to a style that is added stays: {h}");
    let verbatim = style_text(&merged, "VerbatimChar");
    assert!(verbatim.contains("<w:basedOn w:val=\"TextkrperZchn\"/>"), "{verbatim}");
    // a style that is added keeps the id it has when the original lacks its name
    assert!(style_text(&merged, "Title").contains("<w:basedOn w:val=\"Standard\"/>") && style_text(&merged, "Title").contains("<w:next w:val=\"Textkrper\"/>"));
    assert!(reference::merge_styles("<w:styles", &pandoc_styles()).unwrap_err().contains("its styles cannot be read"));
    assert!(reference::merge_styles(&original, "<w:styles").unwrap_err().contains("default styles cannot be read"));
}

// spec: DOCX-007
#[test]
fn the_template_is_a_copy_of_the_original_with_its_parts_and_the_merged_styles() {
    let original_styles = styles_xml("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>");
    let original = zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", "<d>original body</d>"), ("word/styles.xml", &original_styles), ("word/header1.xml", "<h/>"), ("word/media/logo.png", "PNG")]);
    let default = zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())]);
    let made = reference::from_original(&original, &default).unwrap();
    let p = Package::read(&made).unwrap();
    assert_eq!(p.names().collect::<Vec<_>>(), ["[Content_Types].xml", "word/document.xml", "word/styles.xml", "word/header1.xml", "word/media/logo.png"], "the parts of the original");
    assert_eq!(p.text("word/header1.xml").unwrap(), "<h/>");
    assert!(style_ids(p.text("word/styles.xml").unwrap()).contains(&"Heading2".to_string()));
    // what cannot be the template says why
    let e = |o: &[u8], d: &[u8]| reference::from_original(o, d).unwrap_err();
    assert!(e(b"not a zip", &default).contains("original Word file cannot be the template"));
    assert!(e(&zip_of(&[("[Content_Types].xml", "<t/>")]), &default).contains("no word/styles.xml"));
    assert!(e(&zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", "<broken")]), &default).contains("its styles cannot be read"));
    assert!(e(&original, b"not a zip").contains("zip"));
}

// spec: DOCX-005
#[test]
fn the_text_width_is_that_of_the_notes_page_else_of_the_first_section_of_the_reference_else_words_default() {
    let with_section = |inner: &str| zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", &body(&sect(inner)))]);
    let a4 = with_section("<w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1\" w:right=\"1417\" w:bottom=\"1\" w:left=\"1417\" w:header=\"1\" w:footer=\"1\" w:gutter=\"0\"/>");
    let near = |a: f32, b: f32| (a - b).abs() < 0.001;
    assert!(near(reference::text_width(&a4, &PrintLook::default()), (11906.0 - 2.0 * 1417.0) / 1440.0), "the reference's page");
    let letter = PrintLook::of(&note_with("print-paper: Letter\nprint-margins: 25.4 25.4 25.4 25.4"));
    assert!(near(reference::text_width(&a4, &letter), 6.5), "the note's page wins");
    let wide = PrintLook::of(&note_with("print-paper: 297x210mm"));
    assert!(near(reference::text_width(&a4, &wide), 297.0 / 25.4 - 2.0 * 1417.0 / 1440.0), "its paper with the margins of the reference");
    assert!(near(reference::text_width(&a4, &PrintLook::of(&note_with("print-margins: 10 20 10 30"))), (11906.0 - 2.0 * 1417.0) / 1440.0 + 2.0 * 1417.0 / 1440.0 - 50.0 / 25.4), "its margins with the page of the reference");
    for none in [
        zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", &body(&sect("<w:footnotePr/>")))]),
        zip_of(&[("[Content_Types].xml", "<t/>")]),
        b"no zip".to_vec(),
        zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", "<broken")]),
        zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", &body("<w:p/>"))]),
    ] {
        assert!(near(reference::text_width(&none, &PrintLook::default()), 6.5), "Word's own page: 8.5 in with margins of 1 in");
    }
    assert!(near(reference::text_width(&a4, &PrintLook::of(&note_with("print-margins: 100 100 100 100\nprint-paper: 50x50mm"))), 1.0), "never less than an inch");
}

// ---------------------------------------------------------------- DOCX-008: the template from the print settings

/// The note's front matter with these lines.
fn note_with(lines: &str) -> String {
    format!("---\n{lines}\n---\nText\n")
}

const ALL_PROPS: &str = "print-paper: Letter\nprint-margins: 20 25 20 30\nprint-font: serif\nprint-font-size: 12\nprint-line-spacing: 1.15\nprint-paragraph-spacing: 8\nprint-align: justify\nprint-heading-color: \"#2F5496\"\nprint-heading-weight: bold\nprint-heading-sizes: 20 16 14 12 12 12\nprint-break-before: 2";

/// The names of the children of the properties element `kind` of the style `id` (or of `defaults`), as written.
fn children(xml: &str, id: &str, kind: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let holder = match id {
        "pPrDefault" | "rPrDefault" => doc.descendants().find(|n| n.tag_name().name() == id).unwrap(),
        _ => doc.root_element().children().find(|n| n.is_element() && n.attribute((W, "styleId")) == Some(id)).unwrap_or_else(|| panic!("no style {id}")),
    };
    holder.children().find(|n| n.is_element() && n.tag_name().name() == kind).map(|p| p.children().filter(|c| c.is_element()).map(|c| c.tag_name().name().to_string()).collect()).unwrap_or_default()
}

/// An attribute of a child of the properties element `kind` of a style: `attr(xml, "Heading1", "rPr", "sz", "val")`.
fn attr(xml: &str, id: &str, kind: &str, child: &str, attribute: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let holder = match id {
        "pPrDefault" | "rPrDefault" => doc.descendants().find(|n| n.tag_name().name() == id).unwrap(),
        _ => doc.root_element().children().find(|n| n.is_element() && n.attribute((W, "styleId")) == Some(id))?,
    };
    let kind = holder.children().find(|n| n.is_element() && n.tag_name().name() == kind)?;
    kind.children().find(|c| c.is_element() && c.tag_name().name() == child)?.attribute((W, attribute)).map(String::from)
}

// spec: DOCX-008
#[test]
fn every_print_property_is_laid_into_the_styles() {
    let look = PrintLook::of(&note_with(ALL_PROPS));
    let styles = reference::lay_props(&pandoc_styles(), &look).unwrap();
    let a = |id: &str, kind: &str, child: &str, at: &str| attr(&styles, id, kind, child, at);
    // the font of the defaults and of every style that names a theme font; a font by name stays
    for (id, kind) in [("rPrDefault", "rPr"), ("Title", "rPr"), ("Heading1", "rPr"), ("Heading4", "rPr")] {
        assert_eq!(a(id, kind, "rFonts", "ascii").as_deref(), Some("Times New Roman"), "{id}");
        assert_eq!(a(id, kind, "rFonts", "cs").as_deref(), Some("Times New Roman"), "{id}");
        assert_eq!(a(id, kind, "rFonts", "asciiTheme"), None, "no theme font any more in {id}");
    }
    assert_eq!(a("VerbatimChar", "rPr", "rFonts", "ascii").as_deref(), Some("Consolas"));
    assert_eq!(a("rPrDefault", "rPr", "sz", "val").as_deref(), Some("24"), "12 pt");
    assert_eq!(a("rPrDefault", "rPr", "szCs", "val").as_deref(), Some("24"));
    // the spacing: the line from the factor, the space after from the points; Body Text has no spacing of its own
    assert_eq!((a("pPrDefault", "pPr", "spacing", "line").as_deref(), a("pPrDefault", "pPr", "spacing", "lineRule").as_deref(), a("pPrDefault", "pPr", "spacing", "after").as_deref()), (Some("276"), Some("auto"), Some("160")));
    assert!(children(&styles, "BodyText", "pPr").is_empty(), "no paragraph spacing in Body Text");
    assert_eq!(a("FirstParagraph", "pPr", "spacing", "after"), None);
    // justified text, headings left
    assert_eq!(a("Normal", "pPr", "jc", "val").as_deref(), Some("both"));
    assert_eq!(a("Heading1", "pPr", "jc", "val").as_deref(), Some("left"));
    assert_eq!(a("Heading6", "pPr", "jc", "val").as_deref(), Some("left"));
    // the colour of Heading 1 to 9 (those there are) and Title, without the theme it named; the weight; the sizes of 1 to 6
    for id in ["Heading1", "Heading3", "Heading6", "Title"] {
        assert_eq!(a(id, "rPr", "color", "val").as_deref(), Some("2F5496"), "{id}");
        assert_eq!(a(id, "rPr", "color", "themeColor"), None, "{id}");
    }
    assert_eq!(a("Heading2", "rPr", "b", "val"), None);
    assert!(children(&styles, "Heading2", "rPr").contains(&"b".to_string()) && children(&styles, "Heading2", "rPr").contains(&"bCs".to_string()));
    assert_eq!(a("Title", "rPr", "sz", "val").as_deref(), Some("56"), "Title keeps its size");
    for (id, half) in [("Heading1", "40"), ("Heading2", "32"), ("Heading3", "28"), ("Heading4", "24"), ("Heading5", "24"), ("Heading6", "24")] {
        assert_eq!((a(id, "rPr", "sz", "val").as_deref(), a(id, "rPr", "szCs", "val").as_deref()), (Some(half), Some(half)), "{id}");
    }
    // a page break before the headings up to the level
    assert!(children(&styles, "Heading1", "pPr").contains(&"pageBreakBefore".to_string()) && children(&styles, "Heading2", "pPr").contains(&"pageBreakBefore".to_string()));
    assert!(!children(&styles, "Heading3", "pPr").contains(&"pageBreakBefore".to_string()));
    roxmltree::Document::parse(&styles).unwrap();
}

// spec: DOCX-008
#[test]
fn the_properties_are_put_where_the_schema_wants_them() {
    let styles = reference::lay_props(&pandoc_styles(), &PrintLook::of(&note_with(ALL_PROPS))).unwrap();
    assert_eq!(children(&styles, "Heading1", "pPr"), ["keepNext", "keepLines", "pageBreakBefore", "spacing", "jc", "outlineLvl"]);
    assert_eq!(children(&styles, "Heading1", "rPr"), ["rFonts", "b", "bCs", "color", "sz", "szCs"]);
    assert_eq!(children(&styles, "Normal", "pPr"), ["jc"], "a style without properties gets them, after its other elements");
    let normal = style_text(&styles, "Normal");
    assert!(normal.find("<w:qFormat />").unwrap() < normal.find("<w:pPr>").unwrap(), "{normal}");
    assert_eq!(children(&styles, "rPrDefault", "rPr"), ["rFonts", "sz", "szCs"]);
    assert_eq!(children(&styles, "pPrDefault", "pPr"), ["spacing"]);
    // a heading that has no properties element of one kind: it is made before what must follow, e.g. the run properties
    let bare = styles_xml("<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:rPr><w:i/></w:rPr><w:tblPr/></w:style>");
    let laid = reference::lay_props(&bare, &PrintLook::of(&note_with("print-break-before: 1\nprint-heading-weight: bold"))).unwrap();
    assert_eq!(children(&laid, "Heading1", "pPr"), ["pageBreakBefore"]);
    assert!(laid.find("<w:pPr>").unwrap() < laid.find("<w:rPr>").unwrap(), "{laid}");
    assert_eq!(children(&laid, "Heading1", "rPr"), ["b", "bCs", "i"], "bold comes before italic");
    let tabled = styles_xml("<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:tblPr/></w:style>");
    let laid = reference::lay_props(&tabled, &PrintLook::of(&note_with("print-heading-weight: bold\nprint-break-before: 1"))).unwrap();
    assert!(laid.find("<w:rPr>").unwrap() < laid.find("<w:tblPr/>").unwrap() && laid.find("<w:pPr>").unwrap() < laid.find("<w:rPr>").unwrap(), "{laid}");
}

// spec: DOCX-008
#[test]
fn the_fonts_the_weight_and_the_page_break_level_follow_the_property() {
    let lay = |lines: &str| reference::lay_props(&pandoc_styles(), &PrintLook::of(&note_with(lines))).unwrap();
    for (value, name) in [("serif", "Times New Roman"), ("sans", "Calibri"), ("mono", "Courier New"), ("SERIF", "Times New Roman")] {
        assert_eq!(attr(&lay(&format!("print-font: {value}")), "rPrDefault", "rPr", "rFonts", "ascii").as_deref(), Some(name), "{value}");
    }
    let plain = lay("print-heading-weight: normal");
    assert_eq!((attr(&plain, "Heading1", "rPr", "b", "val").as_deref(), attr(&plain, "Heading1", "rPr", "bCs", "val").as_deref()), (Some("0"), Some("0")), "not bold: b is switched off");
    assert_eq!(attr(&plain, "Title", "rPr", "b", "val"), None, "Title keeps its weight");
    let six = lay("print-break-before: 6");
    assert!(children(&six, "Heading6", "pPr").contains(&"pageBreakBefore".to_string()) && !children(&six, "Heading6", "pPr").is_empty());
    let none = lay("print-break-before: 0");
    assert!(!none.contains("pageBreakBefore"), "0 is no level");
    // line spacing and the space after change one attribute of the one element each
    let lines = lay("print-line-spacing: 2");
    assert_eq!((attr(&lines, "pPrDefault", "pPr", "spacing", "line").as_deref(), attr(&lines, "pPrDefault", "pPr", "spacing", "after").as_deref()), (Some("480"), Some("200")), "the space after stays");
    let after = lay("print-paragraph-spacing: 0");
    assert_eq!(attr(&after, "pPrDefault", "pPr", "spacing", "after").as_deref(), Some("0"));
    assert_eq!(attr(&after, "pPrDefault", "pPr", "spacing", "line"), None);
    // left is the default: nothing changes
    assert_eq!(lay("print-align: left"), pandoc_styles());
    // a style that is not there is not made, and defaults without an inside are filled
    let few = styles_xml("<w:docDefaults><w:rPrDefault/></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>");
    let laid = reference::lay_props(&few, &PrintLook::of(&note_with("print-font-size: 10\nprint-heading-color: \"#ff0000\"\nprint-line-spacing: 1.5\nprint-align: justify"))).unwrap();
    assert_eq!(attr(&laid, "rPrDefault", "rPr", "sz", "val").as_deref(), Some("20"));
    assert_eq!(attr(&laid, "Normal", "pPr", "jc", "val").as_deref(), Some("both"));
    assert!(!laid.contains("pPrDefault") && !laid.contains("FF0000"), "no defaults for paragraphs and no headings to change: {laid}");
    let childless = reference::lay_props(&styles_xml("<w:docDefaults><w:pPrDefault></w:pPrDefault></w:docDefaults>"), &PrintLook::of(&note_with("print-line-spacing: 1.5"))).unwrap();
    assert_eq!(attr(&childless, "pPrDefault", "pPr", "spacing", "line").as_deref(), Some("360"));
}

// spec: DOCX-008
#[test]
fn a_bad_value_is_ignored_as_in_the_pdf_export_and_a_note_without_properties_gets_the_default_as_it_is() {
    let bad = note_with("print-font: comic\nprint-font-size: 99\nprint-line-spacing: wide\nprint-paragraph-spacing: -2\nprint-align: right\nprint-heading-color: red\nprint-heading-weight: heavy\nprint-heading-sizes: 1 2\nprint-break-before: 9\nprint-paper: nowhere\nprint-margins: 1 2\nprint-title: false");
    assert_eq!(PrintLook::of(&bad), PrintLook::default());
    assert_eq!(PrintLook::of("Text without front matter\n"), PrintLook::default());
    assert!(!PrintLook::default().styled());
    let default = zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())]);
    assert_eq!(reference::from_props(&default, &PrintLook::of(&bad)).unwrap(), default, "the default as it is, not packed again");
    assert_eq!(reference::lay_props(&pandoc_styles(), &PrintLook::default()).unwrap(), pandoc_styles());
    // the page alone does not touch the styles: it is the file's (DOCX-009)
    let page = PrintLook::of(&note_with("print-paper: A4\nprint-margins: 10 10 10 10"));
    assert!(!page.styled());
    assert_eq!(page.paper, Some([210.0, 297.0]));
    assert_eq!(page.margins, Some([10.0; 4]));
    assert_eq!(PrintLook::of(&note_with("print-paper: 297x210mm")).paper, Some([297.0, 210.0]));
    assert_eq!(PrintLook::of(&note_with("print-paper: Legal")).paper, Some([215.9, 355.6]));
    // the same values as the PDF export reads
    let look = PrintLook::of(&note_with(ALL_PROPS));
    let pdf = crate::pdf::export::ExportOptions::for_note().with_note_properties(&note_with(ALL_PROPS));
    assert_eq!((look.font, look.font_pt, look.line_spacing, look.paragraph_spacing, look.justify), (Some(pdf.font), Some(pdf.font_pt), pdf.line_spacing, pdf.paragraph_spacing, pdf.justify));
    assert_eq!((look.heading_color, look.heading_bold, look.heading_sizes, look.break_before.unwrap(), look.margins), (pdf.heading_color, Some(pdf.heading_bold), pdf.heading_sizes, pdf.break_before, Some(pdf.margins_mm)));
    // a package that is not a Word file is an error
    assert!(reference::from_props(b"no zip", &look).is_err());
    assert!(reference::from_props(&zip_of(&[("[Content_Types].xml", "<t/>")]), &look).unwrap_err().contains("no word/styles.xml"));
    assert!(reference::lay_props("<w:styles", &look).unwrap_err().contains("cannot be read"));
}

// spec: DOCX-008
#[test]
fn the_word_import_gives_the_properties_back() {
    // the import reads the first level that starts a page: 1 comes back as 1 (2 would come back as 1, IMP-044)
    let look = PrintLook::of(&note_with(&ALL_PROPS.replace("print-break-before: 2", "print-break-before: 1")));
    let default = zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())]);
    let made = Package::read(&reference::from_props(&default, &look).unwrap()).unwrap();
    let document = format!("<w:document xmlns:w=\"{W}\"><w:body/></w:document>");
    let read = docx_look::read(&document, Some(made.text("word/styles.xml").unwrap()), None);
    let got: Vec<(String, String)> =
        read.props().into_iter().filter(|(k, _)| !["print-title", "print-toc", "numbered-headings", "print-paper", "print-margins"].contains(k)).map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string())).collect();
    let want: Vec<(String, String)> = [
        ("print-font", "serif"),
        ("print-font-size", "12"),
        ("print-line-spacing", "1.15"),
        ("print-paragraph-spacing", "8"),
        ("print-align", "justify"),
        ("print-heading-color", "#2F5496"),
        ("print-heading-weight", "bold"),
        ("print-heading-sizes", "20 16 14 12 12 12"),
        ("print-break-before", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    assert_eq!(got, want);
}

// ---------------------------------------------------------------- DOCX-009: finishing the file

const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn docx_of(document: &str, styles: &str, rels: &[(&str, &str)], media: &[&str]) -> Vec<u8> {
    let mut parts: Vec<(String, String)> = vec![("[Content_Types].xml".into(), "<Types/>".into()), ("word/document.xml".into(), document.into()), ("word/styles.xml".into(), styles.into())];
    parts.extend(rels.iter().map(|(n, c)| (n.to_string(), c.to_string())));
    parts.extend(media.iter().map(|m| (format!("word/media/{m}"), "PNG".to_string())));
    let files: Vec<(String, Vec<u8>)> = parts.into_iter().map(|(n, c)| (n, c.into_bytes())).collect();
    docx_look::zip_bytes(&files, false).unwrap()
}

fn body(inner: &str) -> String {
    format!("<w:document xmlns:w=\"{W}\" xmlns:r=\"{R}\"><w:body>{inner}</w:body></w:document>")
}

const MARK: &str = "<w:bookmarkEnd w:id=\"0\"/>";

fn mark(align: &str) -> String {
    format!("<w:bookmarkStart w:id=\"0\" w:name=\"_shale_jc_{align}\"/>{MARK}")
}

/// The children of the paragraph number `n` (counting those of the body) as names, `pPr` expanded.
fn paragraph(xml: &str, n: usize) -> (Vec<String>, Vec<String>) {
    let doc = roxmltree::Document::parse(xml).unwrap();
    let p = doc.descendants().filter(|x| x.is_element() && x.tag_name().name() == "p").nth(n).unwrap();
    let names = |x: roxmltree::Node| x.children().filter(|c| c.is_element()).map(|c| format!("{}{}", c.tag_name().name(), c.attribute((W, "val")).map(|v| format!("={v}")).unwrap_or_default())).collect::<Vec<_>>();
    (names(p), p.children().find(|c| c.is_element() && c.tag_name().name() == "pPr").map(names).unwrap_or_default())
}

// spec: DOCX-009, DOCX-006
#[test]
fn a_marked_paragraph_gets_its_jc_in_the_order_of_the_schema_and_the_mark_goes() {
    let doc = body(&format!(
        "<w:p><w:pPr><w:pStyle w:val=\"BodyText\"/></w:pPr>{}<w:r><w:t>centred</w:t></w:r></w:p>\
         <w:p>{}<w:r><w:t>no properties</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"BodyText\"/><w:jc w:val=\"left\"/><w:rPr><w:b/></w:rPr></w:pPr>{}<w:r><w:t>an old one</w:t></w:r></w:p>\
         <w:p><w:pPr><w:pStyle w:val=\"Heading1\"/><w:keepNext/><w:outlineLvl w:val=\"0\"/></w:pPr>{}<w:bookmarkStart w:id=\"21\" w:name=\"heading\"/><w:r><w:t>Head</w:t></w:r><w:bookmarkEnd w:id=\"21\"/></w:p>\
         <w:p>{}<w:r><w:t>unknown</w:t></w:r></w:p>\
         <w:p><w:bookmarkStart w:id=\"0\" w:name=\"_shale_jc_left\"/><w:bookmarkEnd w:id=\"5\"/><w:r><w:t>another end</w:t></w:r></w:p>\
         <w:p><w:r><w:t>plain</w:t></w:r></w:p>",
        mark("center"),
        mark("right"),
        mark("both"),
        mark("left"),
        mark("weird")
    ));
    let made = finish::finish(&docx_of(&doc, &styles_xml(""), &[], &[]), &PrintLook::default()).unwrap();
    let out = part(&made, "word/document.xml");
    assert!(!out.contains("_shale_jc_") || out.matches("_shale_jc_").count() == 1, "only the mark whose end is another bookmark's stays as it was: {out}");
    assert_eq!(paragraph(&out, 0).1, ["pStyle=BodyText", "jc=center"]);
    assert_eq!(paragraph(&out, 0).0, ["pPr", "r"], "the mark is gone, the text stays");
    assert_eq!(paragraph(&out, 1), (vec!["pPr".into(), "r".into()], vec!["jc=right".into()]), "a paragraph without properties gets them first");
    assert_eq!(paragraph(&out, 2).1, ["pStyle=BodyText", "jc=both", "rPr"], "an old jc is replaced, in its place");
    assert_eq!(paragraph(&out, 3).1, ["pStyle=Heading1", "keepNext", "jc=left", "outlineLvl=0"]);
    assert_eq!(paragraph(&out, 3).0, ["pPr", "bookmarkStart", "r", "bookmarkEnd"], "the heading's own bookmark stays");
    assert_eq!(paragraph(&out, 4), (vec!["r".into()], vec![]), "an alignment that Word does not know sets nothing, the mark goes");
    assert_eq!(paragraph(&out, 5).1, ["jc=left"], "a mark whose end is not next to it still gets its paragraph aligned");
    assert_eq!(paragraph(&out, 6), (vec!["r".into()], vec![]));
}

// spec: DOCX-009
#[test]
fn the_character_styles_of_the_colours_get_their_colour_and_fill() {
    let char_style = |name: &str, ty: &str, inner: &str| format!("<w:style w:type=\"{ty}\" w:customStyle=\"1\" w:styleId=\"{}\"><w:name w:val=\"{name}\"/><w:basedOn w:val=\"BodyTextChar\"/>{inner}</w:style>", name.replace(' ', ""));
    let styles = styles_xml(&format!(
        "{}{}{}{}{}{}{}",
        char_style("Shale colour c00000", "character", ""),
        char_style("Shale colour 00B050 on FFC000", "character", ""),
        char_style("Shale on 00ff00", "character", "<w:rPr><w:b/><w:sz w:val=\"20\"/></w:rPr>"),
        char_style("Shale colour zzzzzz", "character", ""),
        char_style("Shale colour C00000 on 12345", "character", ""),
        char_style("Shale colour 123456", "paragraph", ""),
        char_style("Emphasis red", "character", "")
    ));
    let made = finish::finish(&docx_of(&body("<w:p/>"), &styles, &[], &[]), &PrintLook::default()).unwrap();
    let out = part(&made, "word/styles.xml");
    let rpr = |id: &str| children(&out, id, "rPr");
    assert_eq!(rpr("Shalecolourc00000"), ["color"]);
    assert_eq!(attr(&out, "Shalecolourc00000", "rPr", "color", "val").as_deref(), Some("C00000"));
    assert_eq!(rpr("Shalecolour00B050onFFC000"), ["color", "shd"]);
    assert_eq!(
        (attr(&out, "Shalecolour00B050onFFC000", "rPr", "shd", "fill").as_deref(), attr(&out, "Shalecolour00B050onFFC000", "rPr", "shd", "val").as_deref(), attr(&out, "Shalecolour00B050onFFC000", "rPr", "shd", "color").as_deref()),
        (Some("FFC000"), Some("clear"), Some("auto"))
    );
    assert_eq!(rpr("Shaleon00ff00"), ["b", "sz", "shd"], "the fill comes after the size, as the schema wants it");
    assert_eq!(attr(&out, "Shaleon00ff00", "rPr", "shd", "fill").as_deref(), Some("00FF00"));
    for untouched in ["Shalecolourzzzzzz", "ShalecolourC00000on12345", "Shalecolour123456", "Emphasisred"] {
        assert!(rpr(untouched).is_empty(), "{untouched}");
    }
}

fn sect(inner: &str) -> String {
    format!("<w:sectPr>{inner}</w:sectPr>")
}

const BASE_SECT: &str = "<w:headerReference w:type=\"default\" r:id=\"rId9\"/><w:type w:val=\"nextPage\"/><w:pgSz w:w=\"11906\" w:h=\"16838\" w:code=\"9\"/><w:pgMar w:top=\"1\" w:right=\"2\" w:bottom=\"3\" w:left=\"4\" w:header=\"500\" w:footer=\"600\" w:gutter=\"100\"/><w:cols w:space=\"720\"/>";

fn last_section(out: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(out).unwrap();
    let s = doc.root_element().children().find(|n| n.tag_name().name() == "body").unwrap().children().rfind(|n| n.is_element() && n.tag_name().name() == "sectPr").unwrap();
    s.children().filter(|c| c.is_element()).map(|c| out[c.range()].to_string()).collect()
}

// spec: DOCX-009
#[test]
fn the_last_section_gets_the_page_and_the_margins_of_the_note_and_keeps_its_header_footer_and_gutter() {
    let earlier = "<w:p><w:pPr><w:sectPr><w:pgSz w:w=\"1\" w:h=\"2\"/></w:sectPr></w:pPr></w:p>";
    let docx = docx_of(&body(&format!("{earlier}<w:p/>{}", sect(BASE_SECT))), &styles_xml(""), &[], &[]);
    let finish_with = |lines: &str| part(&finish::finish(&docx, &PrintLook::of(&note_with(lines))).unwrap(), "word/document.xml");
    let out = finish_with("print-paper: Letter\nprint-margins: 20 25 20 30");
    let last = last_section(&out);
    assert_eq!(last[0], "<w:headerReference w:type=\"default\" r:id=\"rId9\"/>");
    assert_eq!(last[1], "<w:type w:val=\"nextPage\"/>");
    assert_eq!(last[2], "<w:pgSz w:w=\"12240\" w:h=\"15840\"/>", "Letter, not the page of the first section; the old code is gone");
    assert_eq!(last[3], "<w:pgMar w:top=\"1134\" w:right=\"1417\" w:bottom=\"1134\" w:left=\"1701\" w:header=\"500\" w:footer=\"600\" w:gutter=\"100\"/>", "the distances of the header, the footer and the binding stay");
    assert_eq!(last[4], "<w:cols w:space=\"720\"/>");
    assert!(out.contains("<w:sectPr><w:pgSz w:w=\"1\" w:h=\"2\"/></w:sectPr>"), "an earlier section is left as it is");
    // A4 and a custom size, landscape when wider than tall
    assert_eq!(last_section(&finish_with("print-paper: A4"))[2], "<w:pgSz w:w=\"11906\" w:h=\"16838\"/>");
    assert_eq!(last_section(&finish_with("print-paper: A4"))[3], last_section(&out)[3].replace("1134", "1").replace("1417", "2").replace("1701", "4").replace("w:bottom=\"1\"", "w:bottom=\"3\""), "margins stay when the note has none");
    assert_eq!(last_section(&finish_with("print-paper: 297x210mm"))[2], "<w:pgSz w:w=\"16838\" w:h=\"11906\" w:orient=\"landscape\"/>");
    assert_eq!(last_section(&finish_with("print-paper: A3"))[2], "<w:pgSz w:w=\"16838\" w:h=\"23811\"/>");
    // margins alone keep the page
    let only = last_section(&finish_with("print-margins: 10 10 10 10"));
    assert_eq!((only[2].as_str(), only[3].starts_with("<w:pgMar w:top=\"567\"")), ("<w:pgSz w:w=\"11906\" w:h=\"16838\" w:code=\"9\"/>", true));
    // a section without them gets them, with the usual distances to header, footer and binding
    let bare = docx_of(&body(&sect("<w:cols w:space=\"720\"/>")), &styles_xml(""), &[], &[]);
    let out = part(&finish::finish(&bare, &PrintLook::of(&note_with("print-paper: Letter\nprint-margins: 25.4 25.4 25.4 25.4"))).unwrap(), "word/document.xml");
    assert_eq!(last_section(&out), ["<w:pgSz w:w=\"12240\" w:h=\"15840\"/>", "<w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/>", "<w:cols w:space=\"720\"/>"]);
    // a note without page properties leaves the section alone; a document without a section is no error
    assert_eq!(finish_with("print-font: serif").matches(BASE_SECT).count(), 1);
    let none = docx_of(&body("<w:p/>"), &styles_xml(""), &[], &[]);
    assert!(finish::finish(&none, &PrintLook::of(&note_with("print-paper: Letter"))).is_ok());
    let childless = docx_of(&body("<w:sectPr/>"), &styles_xml(""), &[], &[]);
    assert!(finish::finish(&childless, &PrintLook::of(&note_with("print-paper: Letter"))).is_ok());
}

// spec: DOCX-009
#[test]
fn a_picture_that_its_part_does_not_use_is_left_out_with_its_relationship_and_the_file_is_compressed() {
    let rels = |targets: &str| format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{targets}</Relationships>");
    let image = |id: &str, target: &str| format!("<Relationship Id=\"{id}\" Type=\"image\" Target=\"{target}\"/>");
    let doc_rels = rels(&format!(
        "{}{}{}{}{}{}<Relationship Id=\"r8\" Type=\"styles\" Target=\"styles.xml\"/><Relationship Id=\"r9\" Type=\"image\" Target=\"media/external.png\" TargetMode=\"External\"/>",
        image("r1", "media/used.png"),
        image("r2", "./media/dotted.png"),
        image("r3", "/word/media/absolute.png"),
        image("r5", "../word/media/up.png"),
        image("r6", "media/pandoc-copy.png"),
        image("r7", "media/twice.png")
    ));
    let header_rels = rels(&format!("{}{}", image("r1", "media/header.png"), image("r2", "media/header-copy.png")));
    let document = format!("<w:document xmlns:w=\"{W}\" xmlns:r=\"{R}\"><w:body><w:drawing r:embed=\"r1\"/><w:drawing r:embed=\"r2\"/><w:drawing r:embed=\"r3\"/><w:drawing r:link=\"r5\"/><w:p w:rsidR=\"r6\"/><w:drawing r:embed=\"r7\"/><w:drawing r:embed=\"r7\"/></w:body></w:document>");
    let header = format!("<w:hdr xmlns:w=\"{W}\" xmlns:r=\"{R}\"><w:drawing r:embed=\"r1\"/></w:hdr>");
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), b"<Types/>".to_vec()),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/header1.xml".into(), header.into_bytes()),
        ("word/styles.xml".into(), styles_xml("").into_bytes()),
        ("word/_rels/document.xml.rels".into(), doc_rels.into_bytes()),
        ("word/_rels/header1.xml.rels".into(), header_rels.into_bytes()),
        ("word/_rels/orphan.xml.rels".into(), rels(&image("r1", "media/of-nothing.png")).into_bytes()),
        ("_rels/.rels".into(), rels("<Relationship Id=\"r1\" Type=\"x\" Target=\"word/document.xml\"/>").into_bytes()),
    ];
    for m in ["used", "dotted", "absolute", "external", "up", "pandoc-copy", "twice", "header", "header-copy", "orphan", "of-nothing"] {
        files.push((format!("word/media/{m}.png"), b"PNG".to_vec()));
    }
    let made = finish::finish(&docx_look::zip_bytes(&files, false).unwrap(), &PrintLook::default()).unwrap();
    let mut kept: Vec<String> = names(&made).into_iter().map(|(n, _)| n).filter(|n| n.starts_with("word/media/")).collect();
    kept.sort();
    assert_eq!(
        kept,
        ["word/media/absolute.png", "word/media/dotted.png", "word/media/header.png", "word/media/of-nothing.png", "word/media/twice.png", "word/media/up.png", "word/media/used.png"],
        "the pictures of the document and of the header that they use (also by a path with . and .., from the root, through r:link); a part that is not there keeps all its relationships"
    );
    let doc_rels = part(&made, "word/_rels/document.xml.rels");
    assert!(!doc_rels.contains("pandoc-copy") && doc_rels.contains("media/used.png") && doc_rels.contains("styles.xml") && doc_rels.contains("external.png"), "the relationship goes with the picture; others stay: {doc_rels}");
    roxmltree::Document::parse(&doc_rels).unwrap();
    assert!(!part(&made, "word/_rels/header1.xml.rels").contains("header-copy"));
    let all = names(&made);
    assert_eq!(all[0].0, "[Content_Types].xml", "first");
    assert!(all.iter().filter(|(n, _)| n == "word/document.xml" || n == "word/styles.xml").all(|(_, m)| *m == 8), "the parts are compressed: {all:?}");
    assert!(crate::pdf::tex::archive::read_zip_files(&made).is_ok(), "it reads back");
}

// spec: DOCX-009
#[test]
fn a_file_that_cannot_be_finished_is_an_error() {
    let ok_styles = styles_xml("");
    let e = |bytes: &[u8]| finish::finish(bytes, &PrintLook::default()).unwrap_err();
    assert!(e(b"not a zip").contains("cannot be finished"));
    assert!(e(&zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &ok_styles)])).contains("no word/document.xml"));
    assert!(e(&zip_of(&[("[Content_Types].xml", "<t/>"), ("word/document.xml", &body("<w:p/>"))])).contains("no word/styles.xml"));
    assert!(e(&docx_of("<w:document", &ok_styles, &[], &[])).contains("word/document.xml cannot be read"));
    assert!(e(&docx_of(&body("<w:p/>"), "<w:styles", &[], &[])).contains("word/styles.xml cannot be read"));
    assert!(e(&docx_of(&body("<w:p/>"), &ok_styles, &[("word/_rels/document.xml.rels", "<Relationships")], &[])).contains("document.xml.rels cannot be read"));
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>";
    let broken_part = [("[Content_Types].xml", "<t/>"), ("word/document.xml", &*body("<w:p/>")), ("word/styles.xml", &*ok_styles), ("word/header1.xml", "<w:hdr"), ("word/_rels/header1.xml.rels", rels)];
    assert!(e(&zip_of(&broken_part)).contains("word/header1.xml cannot be read"));
}

// ---------------------------------------------------------------- DOCX-011: nesting too deep

// spec: DOCX-011, DEV-038
#[test]
fn a_part_nested_too_deeply_is_an_error_of_the_finish_and_of_the_merge_of_styles_and_never_an_abort() {
    let deep = |n: usize, open: &str, close: &str| format!("<w:document xmlns:w=\"{W}\" xmlns:r=\"{R}\"><w:body>{}{}</w:body></w:document>", open.repeat(n), close.repeat(n));
    let document = deep(5000, "<w:sdt>", "</w:sdt>");
    let styles = format!("<w:styles xmlns:w=\"{W}\">{}{}</w:styles>", "<w:style>".repeat(5000), "</w:style>".repeat(5000));
    let finish = |document: &str, styles: &str, rels: &[(&str, &str)]| finish::finish(&docx_of(document, styles, rels, &[]), &PrintLook::default()).unwrap_err();
    let ok_styles = styles_xml("");
    for e in [finish(&document, &ok_styles, &[]), finish(&body("<w:p/>"), &styles, &[]), finish(&body("<w:p/>"), &ok_styles, &[("word/_rels/document.xml.rels", &"<a>".repeat(5000))])] {
        assert!(e.starts_with("pandoc's file cannot be finished: ") && e.contains("the XML is nested too deeply (more than 256 levels)"), "{e}");
    }
    let e = reference::merge_styles(&styles, &ok_styles).unwrap_err();
    assert_eq!(e, "its styles cannot be read: the XML is nested too deeply (more than 256 levels)");
    let e = reference::merge_styles(&ok_styles, &styles).unwrap_err();
    assert_eq!(e, "pandoc's default styles cannot be read: the XML is nested too deeply (more than 256 levels)");
}

// ---------------------------------------------------------------- DOCX-010: the schema check

/// A Word file of a document with the paragraph properties `ppr` and an empty list of styles.
fn docx_with_ppr(ppr: &str) -> Vec<u8> {
    docx_of(&body(&format!("<w:p><w:pPr>{ppr}</w:pPr></w:p>")), &styles_xml(""), &[], &[])
}

// spec: DOCX-010, DEV-036
#[test]
fn the_schemas_are_looked_for_in_the_cache_folder_of_the_developer_tools() {
    let dir = |xdg, home| schema_check::cache_dir(xdg, home);
    // joined part by part: Windows puts its own separator between the parts
    let under = |base: &std::path::Path| base.join("shale-dev").join("docx-schemas").join(schema_check::COMMIT);
    let cache = std::path::Path::new("/h").join(".cache");
    assert_eq!(dir(Some("/x"), Some("/h")), Some(under(std::path::Path::new("/x"))), "XDG_CACHE_HOME first");
    assert_eq!(dir(Some(""), Some("/h")), Some(under(&cache)), "an empty one is not set");
    assert_eq!(dir(None, Some("/h")), Some(under(&cache)));
    assert_eq!(dir(None, Some("")), None);
    assert_eq!(dir(None, None), None);
}

// spec: DOCX-010, DEV-036
#[test]
fn the_commit_of_the_schemas_is_the_one_the_script_fetches() {
    let script = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/validate-docx.sh")).unwrap();
    assert!(script.lines().any(|l| l == format!("COMMIT={}", schema_check::COMMIT)), "COMMIT in scripts/validate-docx.sh and schema_check::COMMIT differ");
}

// spec: DOCX-010, DEV-036
#[test]
fn a_check_without_the_schemas_is_skipped_and_what_is_no_word_file_is_invalid() {
    let empty = temp_dir("no-schemas");
    match schema_check::check_with(&docx_with_ppr(""), &empty) {
        Outcome::Skipped(why) => assert!(why.contains("xmllint is not installed") || why.contains("are not in"), "{why}"),
        other => panic!("{other:?}"),
    }
    // the cache is only used when the script marked it as verified
    std::fs::create_dir_all(empty.join("schemas/microsoft")).unwrap();
    std::fs::write(empty.join("schemas/microsoft/wml-2010.xsd"), "<xsd/>").unwrap();
    assert!(matches!(schema_check::check_with(&docx_with_ppr(""), &empty), Outcome::Skipped(_)));
    assert!(matches!(schema_check::check_with(b"not a zip", &empty), Outcome::Invalid(why) if why.starts_with("not a Word file")));
    let _ = std::fs::remove_dir_all(&empty);
}

// spec: DOCX-010, DEV-036
#[test]
fn a_file_with_elements_in_the_wrong_order_does_not_validate_and_the_errors_name_the_part() {
    let good = schema_check::check(&docx_with_ppr("<w:keepNext/><w:spacing w:after=\"0\"/><w:jc w:val=\"left\"/>"));
    if let Outcome::Skipped(why) = &good {
        eprintln!("skipping: {why}");
        return;
    }
    assert_eq!(good, Outcome::Valid);
    // the order of the schema is pageBreakBefore, spacing, jc: this is the failure of the old styled.docx
    let Outcome::Invalid(errors) = schema_check::check(&docx_with_ppr("<w:spacing w:after=\"0\"/><w:pageBreakBefore/>")) else { panic!("it validates") };
    assert!(errors.starts_with("word/document.xml:") && errors.contains("w:pageBreakBefore") && errors.contains("not expected"), "{errors}");
    assert!(!errors.contains("http://schemas"), "the namespace is shortened: {errors}");
    // a part that is no XML
    let broken = docx_of(&body("<w:p/>"), "<w:styles", &[], &[]);
    let Outcome::Invalid(errors) = schema_check::check(&broken) else { panic!("it validates") };
    assert!(errors.starts_with("word/styles.xml: not well-formed XML"), "{errors}");
}

// The Word files of the import's tests are valid WordprocessingML (IMP-039), except what LibreOffice wrote.
// spec: DOCX-010, IMP-039
#[test]
fn every_fixture_of_the_import_validates() {
    // toc.docx is made by LibreOffice, whose styles.xml and numbering.xml carry `mc:Ignorable` on the root, which the
    // schema does not declare for them (markup compatibility says every reader has to accept it); out of our hands.
    let by_libreoffice = ["toc.docx"];
    let mut seen = 0;
    for entry in std::fs::read_dir(fixture("")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "docx") || by_libreoffice.contains(&path.file_name().unwrap().to_str().unwrap()) {
            continue;
        }
        seen += 1;
        schema_check::assert_valid(&std::fs::read(&path).unwrap(), &path.display().to_string());
    }
    assert!(seen >= 5, "the fixtures: {seen}");
}

// ---------------------------------------------------------------- DOCX-010: the round trip with the real pandoc

/// The share of the words of the fixture `name` that come back through import and export, and the trip.
fn fixture_trip(name: &str) -> Option<(f64, Trip)> {
    let pandoc = real_pandoc()?;
    let file = fixture(&format!("{name}.docx"));
    let trip = round_trip(name, &file)?;
    schema_check::assert_valid(&trip.bytes, &format!("the export of {name}"));
    Some((share(&words_of(&pandoc, &file), &words_of(&pandoc, &trip.exported)), trip))
}

// spec: DOCX-010
#[test]
fn the_words_of_styled_picture_and_tables_all_come_back_and_those_of_text_nearly_all() {
    for (name, least) in [("styled", 1.0), ("picture", 1.0), ("tables", 1.0), ("text", 0.95)] {
        let Some((got, _)) = fixture_trip(name) else { return };
        assert!(got >= least, "{name}: {:.1} % of the words came back, at least {:.0} % is the rule", got * 100.0, least * 100.0);
    }
}

// spec: DOCX-010, DOCX-004, DOCX-006, DOCX-009
#[test]
fn styled_comes_back_with_its_colours_highlights_page_breaks_title_and_paper() {
    let Some((_, trip)) = fixture_trip("styled") else { return };
    let (document, styles) = (part(&trip.bytes, "word/document.xml"), part(&trip.bytes, "word/styles.xml"));
    assert_eq!(attr(&styles, "ShalecolourC00000", "rPr", "color", "val").as_deref(), Some("C00000"), "the red text");
    assert_eq!(attr(&styles, "Shalecolour00B050onFFC000", "rPr", "shd", "fill").as_deref(), Some("FFC000"), "green on orange");
    assert_eq!(attr(&styles, "Shalecolour00B050onFFC000", "rPr", "color", "val").as_deref(), Some("00B050"));
    assert_eq!(attr(&styles, "Shaleon00FF00", "rPr", "shd", "fill").as_deref(), Some("00FF00"), "the green highlight");
    assert!(document.contains("<w:rStyle w:val=\"ShalecolourC00000\"") && document.contains("<w:rStyle w:val=\"Shaleon00FF00\""), "{document}");
    assert_eq!(document.matches("<w:highlight w:val=\"yellow\"").count(), 1, "the yellow highlight is Word's own");
    assert_eq!(document.matches("<w:br w:type=\"page\"/>").count(), 3, "the three page breaks");
    assert!(document.contains("<w:pStyle w:val=\"Title\"") && document.contains("<w:pStyle w:val=\"Subtitle\""), "the title block is Word's Title and Subtitle");
    assert!(document.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"), "Letter paper, not the page of the first section");
    assert!(!document.contains("_shale_jc_") && !trip.text.is_empty());
    // the note's look is the original's: its numbers are in the last section
    assert!(document.contains("w:top=\"1134\" w:right=\"1417\" w:bottom=\"1134\" w:left=\"1701\""), "{document}");
}

// spec: DOCX-010, DOCX-005
#[test]
fn the_picture_comes_back_as_a_picture_with_its_caption_and_alternative_text_and_the_originals_pictures_are_not_copied() {
    let Some((_, trip)) = fixture_trip("picture") else { return };
    let document = part(&trip.bytes, "word/document.xml");
    assert_eq!(document.matches("<w:drawing>").count(), 1, "{document}");
    assert!(document.contains("descr=\"A red square\""), "the caption is the alternative text too");
    assert!(document.contains("<w:pStyle w:val=\"ImageCaption\"") && document.contains("<w:pStyle w:val=\"CaptionedFigure\""), "a figure with its caption");
    let media: Vec<String> = names(&trip.bytes).into_iter().map(|(n, _)| n).filter(|n| n.starts_with("word/media/")).collect();
    assert_eq!(media.len(), 1, "the picture of the note, not the one of the template too: {media:?}");
}

// spec: DOCX-010
#[test]
fn tables_come_back_as_tables_and_a_german_original_is_a_template_pandoc_can_use() {
    let Some((_, trip)) = fixture_trip("tables") else { return };
    let document = part(&trip.bytes, "word/document.xml");
    assert_eq!(document.matches("<w:tbl>").count(), 1);
    assert!(document.contains("<w:tblStyle w:val=\"Table\""), "the table has a style that the file has");
    assert!(style_ids(&part(&trip.bytes, "word/styles.xml")).contains(&"Table".to_string()));
}

// ---------------------------------------------------------------- DOCX-007: a Word file of Word as the template

// spec: DOCX-007
#[test]
fn the_original_of_a_word_file_is_the_template_with_its_styles_and_the_styles_pandoc_needs_merged_in() {
    let Some(pandoc) = real_pandoc() else { return };
    let dir = temp_dir("template");
    // a Word file as Word makes it: German Normal, no style of pandoc's
    let from = Package::read(&crate::fsutil::read(fixture("text.docx")).unwrap()).unwrap();
    let styles = styles_xml(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Georgia\" w:hAnsi=\"Georgia\"/><w:sz w:val=\"22\"/></w:rPr></w:rPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Standard\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"berschrift1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Standard\"/><w:next w:val=\"Standard\"/><w:qFormat/><w:rPr><w:b/><w:color w:val=\"1F3864\"/><w:sz w:val=\"36\"/></w:rPr></w:style>\
         <w:style w:type=\"character\" w:default=\"1\" w:styleId=\"Absatz-Standardschriftart\"><w:name w:val=\"Default Paragraph Font\"/></w:style>\
         <w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"MeinStil\"><w:name w:val=\"Mein Stil\"/><w:basedOn w:val=\"Standard\"/></w:style>",
    );
    let mut parts: Vec<(String, Vec<u8>)> = from.names().map(|n| (n.to_string(), from.text(n).map(|t| t.as_bytes().to_vec()).unwrap_or_default())).filter(|(n, _)| n != "word/styles.xml").collect();
    parts.push(("word/styles.xml".into(), styles.into_bytes()));
    std::fs::write(dir.join("Report.docx"), docx_look::zip_bytes(&parts.into_iter().filter(|(n, _)| !n.contains("media/")).collect::<Vec<_>>(), false).unwrap()).unwrap();
    std::fs::write(dir.join("Note.md"), "---\nsource: \"[[Report.docx]]\"\n---\n\n# Head\n\nText with a list:\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
    let vault = Vault::open(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("Note.md")).unwrap();
    let original = original_of(&vault, Path::new("Note.md"), &text);
    assert_eq!(original, Some(PathBuf::from("Report.docx")));
    let job = Job::new(pandoc.clone(), &vault, Path::new("Note.md"), &text, original.as_deref(), false, true);
    let bytes = export_valid(&job);
    let styles = part(&bytes, "word/styles.xml");
    let ids = style_ids(&styles);
    for kept in ["Standard", "berschrift1", "MeinStil"] {
        assert!(ids.contains(&kept.to_string()), "{kept} of the original: {ids:?}");
    }
    for merged in ["Compact", "Table", "BodyText", "FirstParagraph"] {
        assert!(ids.contains(&merged.to_string()), "{merged} of pandoc's: {ids:?}");
    }
    assert!(!ids.contains(&"Normal".to_string()) && !ids.contains(&"Heading1".to_string()), "no second Normal or Heading 1: {ids:?}");
    assert!(style_text(&styles, "BodyText").contains("<w:basedOn w:val=\"Standard\""), "based on the Normal of the original");
    assert_eq!(attr(&styles, "rPrDefault", "rPr", "rFonts", "ascii").as_deref(), Some("Georgia"), "its fonts");
    assert!(!style_text(&styles, "BodyText").contains("spacing"), "the body keeps the original's own look");
    let document = part(&bytes, "word/document.xml");
    assert!(document.contains("<w:pStyle w:val=\"berschrift1\"") || document.contains("<w:pStyle w:val=\"Heading1\""), "{document}");
    assert!(
        words_of(&pandoc, &dir.join("Report.docx")).len() > 10
            && !words_of(&pandoc, &{
                std::fs::write(dir.join("out.docx"), &bytes).unwrap();
                dir.join("out.docx")
            })
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- DOCX-004 to DOCX-006: the filter

/// The picture of the `picture` fixture, as a file `name` in `dir`.
fn put_picture(dir: &Path, name: &str) {
    let word = Package::read(&crate::fsutil::read(fixture("picture.docx")).unwrap()).unwrap();
    let files = crate::pdf::tex::archive::read_zip_files(&crate::fsutil::read(fixture("picture.docx")).unwrap()).unwrap();
    let _ = word;
    let png = files.into_iter().find(|(n, _)| n.starts_with("word/media/")).unwrap().1;
    std::fs::write(dir.join(name), png).unwrap();
}

/// What pandoc made of a note's text: `document.xml` and `styles.xml`.
struct Made {
    document: String,
    styles: String,
    numbering: String,
}

/// Export `md` (what the text is after the preparation) from a vault folder that has the picture `a.png` and `B.PNG`.
fn word(md: &str, toc: bool) -> Option<Made> {
    let pandoc = real_pandoc()?;
    let dir = temp_dir("filter");
    put_picture(&dir, "a.png");
    put_picture(&dir, "B.PNG");
    let job = Job { pandoc, root: dir.clone(), text: md.to_string(), look: PrintLook::default(), original: None, toc, timeout: TIMEOUT, thesis: None, warnings: vec![] };
    let bytes = export_valid(&job);
    let _ = std::fs::remove_dir_all(&dir);
    Some(Made { document: part(&bytes, "word/document.xml"), styles: part(&bytes, "word/styles.xml"), numbering: part(&bytes, "word/numbering.xml") })
}

/// The text of the document in one string, paragraphs separated by `|`.
fn text_of(document: &str) -> String {
    let doc = roxmltree::Document::parse(document).unwrap();
    doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "p").map(|p| p.descendants().filter(|n| n.is_element() && n.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>()).collect::<Vec<_>>().join("|")
}

/// The paragraphs of the document: (their text, whether `w:jc` and which).
fn alignments(document: &str) -> Vec<(String, Option<String>)> {
    let doc = roxmltree::Document::parse(document).unwrap();
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| {
            (
                p.descendants().filter(|n| n.is_element() && n.tag_name().name() == "t").filter_map(|t| t.text()).collect::<String>(),
                p.descendants().find(|n| n.is_element() && n.tag_name().name() == "jc").and_then(|j| j.attribute((W, "val"))).map(String::from),
            )
        })
        .collect()
}

// spec: DOCX-004
#[test]
fn a_mark_and_a_yellow_background_are_words_yellow_highlight() {
    let Some(m) = word("==one== <mark>two</mark> <mark style=\"background: #FFFF00;\">three</mark> ==four five== **==bold==**\n", false) else { return };
    assert_eq!(m.document.matches("<w:highlight w:val=\"yellow\"").count(), 5, "{}", m.document);
    assert_eq!(text_of(&m.document), "one two three four five bold");
    assert!(!m.document.contains("==") && !m.styles.contains("Shaleon"), "no colour style for yellow");
}

// spec: DOCX-004
#[test]
fn a_text_colour_and_a_background_are_a_character_style_of_the_colours_and_other_formatting_stays_inside() {
    let md = "<span style=\"color: #c00000;\">red **strong** and *slanted*</span> <mark style=\"background: #00ff00;\">green</mark> \
<mark style=\"background: #00FF00;\"><span style=\"color: #FF0000;\">both</span></mark> <span style=\"color: #0000FF;\"><mark style=\"background: #EEEEEE;\">other way</mark></span> \
<span style=\"color: #00B050;\"><mark>yellow in colour</mark></span> <span style=\"font-size: 20px\">no colour</span> <span style=\"color: red\">no hex</span>\n";
    let Some(m) = word(md, false) else { return };
    let run_with = |style: &str| {
        roxmltree::Document::parse(&m.document)
            .unwrap()
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "r")
            .filter(|r| r.descendants().any(|s| s.tag_name().name() == "rStyle" && s.attribute((W, "val")) == Some(style)))
            .map(|r| m.document[r.range()].to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(attr(&m.styles, "ShalecolourC00000", "rPr", "color", "val").as_deref(), Some("C00000"), "the colour is upper case in the style");
    let red = run_with("ShalecolourC00000");
    assert_eq!(red.len(), 7, "red, strong, and, slanted and the spaces between: {red:?}");
    assert!(red.iter().any(|r| r.contains("<w:b />") || r.contains("<w:b/>")) && red.iter().any(|r| r.contains("<w:i />") || r.contains("<w:i/>")), "bold and italic stay inside the colour");
    assert_eq!(attr(&m.styles, "Shaleon00FF00", "rPr", "shd", "fill").as_deref(), Some("00FF00"));
    assert_eq!(run_with("Shaleon00FF00").len(), 1, "green background");
    assert_eq!(attr(&m.styles, "ShalecolourFF0000on00FF00", "rPr", "shd", "fill").as_deref(), Some("00FF00"), "a colour inside a background is one style of both");
    assert_eq!(attr(&m.styles, "ShalecolourFF0000on00FF00", "rPr", "color", "val").as_deref(), Some("FF0000"));
    assert_eq!(run_with("ShalecolourFF0000on00FF00").len(), 1);
    assert_eq!(run_with("Shalecolour0000FFonEEEEEE").len(), 1, "and the other way round");
    let yellow = run_with("Shalecolour00B050");
    assert!(yellow.len() == 1 && yellow[0].contains("<w:highlight w:val=\"yellow\""), "a yellow background in a colour: both the style and the highlight: {yellow:?}");
    assert_eq!(text_of(&m.document), "red strong and slanted green both other way yellow in colour no colour no hex", "the words are all there");
    assert!(!m.document.contains("Shalecolourred") && !m.styles.contains("Shalecolour20px"), "a span without a colour in hex is no colour style");
}

// spec: DOCX-004
#[test]
fn underline_sub_and_superscript_strike_through_and_line_breaks() {
    let Some(m) = word("a <u>under</u> b H<sub>2</sub>O x<sup>2</sup> <s>gone</s> ~~away~~ c<br>d e<br/>f\ng\n", false) else { return };
    let d = &m.document;
    assert_eq!(d.matches("<w:u w:val=\"single\"").count(), 1, "{d}");
    assert_eq!((d.matches("w:val=\"subscript\"").count(), d.matches("w:val=\"superscript\"").count()), (1, 1));
    assert_eq!(d.matches("<w:strike").count(), 2, "<s> and ~~");
    assert_eq!(d.matches("<w:br />").count() + d.matches("<w:br/>").count(), 3, "<br>, <br/> and a new line");
    assert_eq!(text_of(d), "a under b H2O x2 gone away cd efg");
}

// spec: DOCX-004
#[test]
fn a_tag_without_its_end_is_left_out_and_a_link_to_a_note_is_its_words() {
    let Some(m) = word("x <u>y and <mark>z and w</u> [[Some note]] [[Some note|words]] [[Folder/Note#Part]] [[a.md]] [[Note#H|alias]]\n", false) else { return };
    let t = text_of(&m.document);
    assert!(t.starts_with("x y and z and w "), "{t}");
    assert!(t.ends_with("Some note words Folder/Note > Part a alias"), "{t}");
    assert!(!m.document.contains("<w:highlight") && !t.contains("[[") && !t.contains("<u>"), "{t}");
}

// spec: DOCX-004
#[test]
fn footnotes_tables_lists_tasks_quotes_code_math_and_web_links_are_pandocs_own() {
    let md = "Text[^1] and [web](https://example.com/a) and https://example.org/bare and $x^2$.\n\n[^1]: The footnote.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] open\n- [x] done\n\n1. one\n2. two\n\n> quoted\n\n```\ncode line\n```\n\n$$\\int_0^1 x\\,dx$$\n";
    let Some(m) = word(md, false) else { return };
    let d = &m.document;
    assert!(d.contains("<w:footnoteReference"), "{d}");
    assert_eq!(d.matches("<w:tbl>").count(), 1);
    assert!(m.numbering.contains("☐") && m.numbering.contains("☒"), "tasks are bullets of boxes");
    assert!(d.matches("<w:numPr>").count() >= 4, "lists");
    assert!(d.contains("<w:pStyle w:val=\"BlockText\""), "a quote");
    assert!(d.contains("w:val=\"VerbatimChar\"") || d.contains("SourceCode"), "code");
    assert!(d.contains("<m:oMath>") && d.contains("<m:oMathPara>"), "math, inline and display");
    assert_eq!(d.matches("<w:hyperlink").count(), 2, "web links");
}

// spec: DOCX-002
#[test]
fn the_table_of_contents_is_there_when_asked_down_to_the_third_level() {
    let md = "# One\n\n## Two\n\n### Three\n\n#### Four\n\ntext\n";
    let Some(with) = word(md, true) else { return };
    assert!(with.document.contains("TOC \\o &quot;1-3&quot;") || with.document.contains("TOC \\o \"1-3\""), "{}", with.document);
    let Some(without) = word(md, false) else { return };
    assert!(!without.document.contains("TOC \\o"));
}

/// The Word table of contents as pandoc writes it, then `after`.
fn with_toc(after: &str) -> String {
    let sdt = "<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val=\"Table of Contents\"/><w:docPartUnique/></w:docPartObj></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Contents</w:t></w:r></w:p></w:sdtContent></w:sdt>";
    body(&format!("{sdt}{after}<w:sectPr/>"))
}

fn break_made(document: &str, on: Option<bool>) -> String {
    let look = PrintLook { toc_page_break: on, ..PrintLook::default() };
    part(&finish::finish(&docx_of(document, &styles_xml(""), &[], &[]), &look).unwrap(), "word/document.xml")
}

// spec: DOCX-013
#[test]
fn the_paragraph_after_the_table_of_contents_starts_a_page_and_only_when_asked() {
    // past the bookmarks of the headings, the paragraph after the contents: with and without properties, in the order of the schema
    let plain = with_toc("<w:bookmarkStart w:id=\"1\" w:name=\"one\"/><w:p><w:r><w:t>One</w:t></w:r></w:p><w:bookmarkEnd w:id=\"1\"/>");
    let out = break_made(&plain, Some(true));
    assert!(out.contains("</w:sdt><w:bookmarkStart w:id=\"1\" w:name=\"one\"/><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r>"), "{out}");
    let styled = with_toc("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/><w:keepNext/><w:outlineLvl w:val=\"0\"/></w:pPr><w:r><w:t>One</w:t></w:r></w:p>");
    let out = break_made(&styled, Some(true));
    assert!(out.contains("<w:pPr><w:pStyle w:val=\"Heading1\"/><w:keepNext/><w:pageBreakBefore/><w:outlineLvl w:val=\"0\"/></w:pPr>"), "{out}");
    // a paragraph that has it already stays as it is: one break, no empty page
    let again = with_toc("<w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>One</w:t></w:r></w:p>");
    assert_eq!(break_made(&again, Some(true)).matches("pageBreakBefore").count(), 1);
    // a table next: a paragraph with a page break between; the end of the body: nothing to break to
    let table = with_toc("<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>");
    assert!(break_made(&table, Some(true)).contains("</w:sdt><w:p><w:r><w:br w:type=\"page\"/></w:r></w:p><w:tbl>"));
    assert_eq!(break_made(&with_toc(""), Some(true)), with_toc(""));
    // off, not said, or no contents: the file is as pandoc made it
    for on in [Some(false), None] {
        assert_eq!(break_made(&plain, on), plain);
    }
    let none = body("<w:p><w:r><w:t>One</w:t></w:r></w:p>");
    assert_eq!(break_made(&none, Some(true)), none, "no table of contents, no break");
    // a paragraph that is centred gets both, and the file stays well formed
    let centred = with_toc(&format!("<w:p>{}<w:r><w:t>One</w:t></w:r></w:p>", mark("center")));
    let out = break_made(&centred, Some(true));
    assert!(out.contains("<w:pPr><w:pageBreakBefore/><w:jc w:val=\"center\"/></w:pPr>") && roxmltree::Document::parse(&out).is_ok(), "{out}");
}

// spec: DOCX-013
#[test]
fn the_note_and_the_dialog_say_whether_the_table_of_contents_is_followed_by_a_page_break() {
    let said = |v: &str| PrintLook::of(&format!("---\nprint-toc-page-break: {v}\n---\nx")).toc_page_break;
    assert_eq!((said("false"), said("yes"), said("maybe")), (Some(false), Some(true), None));
    assert_eq!(PrintLook::of("x").toc_page_break, None);
    let dir = temp_dir("tocbreak");
    std::fs::write(dir.join("N.md"), "x").unwrap();
    let vault = Vault::open(&dir).unwrap();
    let job = |toc: bool, toc_break: bool, text: &str| Job::new(PathBuf::from("pandoc"), &vault, Path::new("N.md"), text, None, toc, toc_break).look.toc_page_break;
    // the dialog's box wins over the note, and there is no break without the contents
    assert_eq!((job(true, true, "x"), job(true, false, "x"), job(false, true, "x")), (Some(true), Some(false), Some(false)));
    assert_eq!(job(true, true, "---\nprint-toc-page-break: false\n---\nx"), Some(true));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: DOCX-013, DOCX-002
#[test]
fn pandocs_table_of_contents_is_followed_by_a_page_break_by_default_and_not_when_off() {
    let md = "# One\n\nfiller text\n\n## Two\n";
    let Some(pandoc) = real_pandoc() else { return };
    let run = |toc: bool, toc_break: bool| {
        let dir = temp_dir("tocbreak-run");
        let look = PrintLook { toc_page_break: Some(toc && toc_break), ..PrintLook::default() };
        let job = Job { pandoc: pandoc.clone(), root: dir.clone(), text: md.to_string(), look, original: None, toc, timeout: TIMEOUT, thesis: None, warnings: vec![] };
        let bytes = export_valid(&job);
        let _ = std::fs::remove_dir_all(&dir);
        part(&bytes, "word/document.xml")
    };
    let on = run(true, true);
    let (toc_end, first) = (on.find("</w:sdt>").unwrap(), on.find(">One<").unwrap());
    assert!(on[toc_end..first].contains("<w:pageBreakBefore/>"), "the heading after the contents starts a page: {on}");
    assert_eq!(on.matches("pageBreakBefore").count(), 1);
    assert!(!run(true, false).contains("pageBreakBefore"), "box off");
    assert!(!run(false, true).contains("pageBreakBefore"), "no contents");
}

// spec: DOCX-005
#[test]
fn a_picture_is_as_wide_as_in_the_editor_and_other_embeds_are_their_names_in_italics() {
    let Some(m) = word("![[a.png]]\n\n![[a.png|360]]\n\n![[a.png|1000]]\n\n![[a.png|A red square]]\n\n![[B.PNG|90x30]]\n\ntext\n\n![[Notes/Other note.md]] ![[Other.md#Part]] ![[doc.pdf]] ![[sound.mp3|3]]\n", false) else { return };
    let doc = roxmltree::Document::parse(&m.document).unwrap();
    let extents: Vec<u64> = doc.descendants().filter(|n| n.tag_name().name() == "extent").map(|e| e.attribute("cx").unwrap().parse().unwrap()).collect();
    let column = 5_943_600u64; // 6.5 in
    assert_eq!(extents.len(), 5, "five pictures: {extents:?}");
    assert_ne!(extents[0], column / 2, "without a width the picture keeps its own size");
    assert!(extents[0] < column / 2);
    assert_eq!(extents[1], column / 2, "360 of the 720 points of the editor's column is half of the text");
    assert!(extents[2] > extents[1] && extents[2] <= column, "wider, never wider than the text: {extents:?}");
    assert_eq!(extents[3], extents[0], "words are the alternative text, not a size");
    assert_eq!(extents[4], column / 8, "90 of 720: an eighth; the height of 90x30 is not asked");
    assert!(m.document.contains("descr=\"A red square\""));
    let t = text_of(&m.document);
    assert!(t.ends_with("text|Other note Other doc.pdf sound.mp3"), "{t}");
    let italics = doc.descendants().filter(|r| r.tag_name().name() == "r" && r.children().any(|p| p.tag_name().name() == "rPr" && p.children().any(|i| i.tag_name().name() == "i"))).count();
    assert_eq!(italics, 4, "each name in italics");
}

// spec: DOCX-005
#[test]
fn a_picture_alone_followed_by_an_italic_paragraph_is_a_picture_with_its_caption() {
    let Some(m) = word("![[a.png]]\n\n*Figure one*\n\ntext\n\n![[a.png]]\n\n*Not* a caption\n\n![[a.png]] beside text\n\n*Alone*\n\n![[a.png]]\n\ntext\n", false) else { return };
    let d = &m.document;
    assert_eq!(d.matches("<w:pStyle w:val=\"ImageCaption\"").count(), 1, "{d}");
    assert_eq!(d.matches("<w:pStyle w:val=\"CaptionedFigure\"").count(), 1);
    assert!(d.contains("descr=\"Figure one\""), "the caption is the picture's alternative text");
    assert_eq!(d.matches("<w:drawing>").count(), 4);
    assert_eq!(d.matches("descr=\"\"").count(), 3, "the others have none");
    assert_eq!(text_of(d), "|Figure one|text||Not a caption| beside text|Alone||text", "the caption is a paragraph of the figure, the picture has none");
}

// spec: DOCX-006
#[test]
fn a_page_break_marker_is_a_page_break() {
    let Some(m) = word("one\n\n<div style=\"page-break-after: always;\"></div>\n\ntwo\n\n<div style=\"break-before: page\"></div>\n\nthree\n\n<div style=\"color: red\"></div>\n\nfour\n", false) else { return };
    assert_eq!(m.document.matches("<w:br w:type=\"page\"/>").count(), 2, "{}", m.document);
    assert_eq!(text_of(&m.document), "one||two||three|four", "a break is a paragraph with nothing but the break");
}

// spec: DOCX-006
#[test]
fn paragraphs_and_headings_in_an_alignment_wrapper_are_aligned_and_the_wrapper_lines_are_no_text() {
    let md = "plain\n\n<div align=\"center\">\n\ncentred\n\n## A heading\n\n</div>\n\n<div align=\"right\">\n\nright\n\n</div>\n\n<div align=\"justify\">\n\njustified\n\n</div>\n\n<div align=\"left\">\n\nleft\n\n</div>\n\nafter\n\n<div align=\"center\">\n\nnever closed\n\nstill\n";
    let Some(m) = word(md, false) else { return };
    let a = alignments(&m.document);
    let of = |t: &str| a.iter().find(|(x, _)| x == t).unwrap_or_else(|| panic!("no {t:?} in {a:?}")).1.clone();
    assert_eq!(of("plain"), None);
    assert_eq!(of("centred").as_deref(), Some("center"));
    assert_eq!(of("A heading").as_deref(), Some("center"));
    assert_eq!(of("right").as_deref(), Some("right"));
    assert_eq!(of("justified").as_deref(), Some("both"));
    assert_eq!(of("left").as_deref(), Some("left"));
    assert_eq!(of("after"), None);
    assert_eq!((of("never closed"), of("still")), (None, None), "an opening line without its closing line aligns nothing");
    let t = text_of(&m.document);
    assert!(!t.contains("div") && !t.contains("align") && !m.document.contains("_shale_jc_"), "the wrapper lines are never text, the marks are gone: {t}");
}

// spec: DOCX-006
#[test]
fn the_title_block_the_import_writes_is_words_title_and_subtitle() {
    let Some(m) = word("<div align=\"center\">\n\n# Report\n\n*Of the year*\n\n</div>\n\n# First heading\n\ntext\n", false) else { return };
    let d = &m.document;
    assert!(d.contains("<w:pStyle w:val=\"Title\"") && d.contains("<w:pStyle w:val=\"Subtitle\"") && d.contains("<w:pStyle w:val=\"Heading1\""), "{d}");
    assert_eq!(d.matches("<w:pStyle w:val=\"Heading1\"").count(), 1, "the title is not a heading");
    assert_eq!(text_of(d), "Report|Of the year|First heading|text");
    assert!(!d.contains("<w:jc w:val=\"center\""), "the style centres it");
    // without a subtitle
    let Some(m) = word("<div align=\"center\">\n\n# Only\n\n</div>\n\ntext\n", false) else { return };
    assert!(m.document.contains("<w:pStyle w:val=\"Title\"") && !m.document.contains("Subtitle") && !m.document.contains("Heading1"), "{}", m.document);
    // something else in the wrapper, or the wrapper not first, or not centred: no title
    for md in [
        "<div align=\"center\">\n\n# T\n\nplain words\n\n</div>\n",
        "text\n\n<div align=\"center\">\n\n# T\n\n</div>\n",
        "<div align=\"right\">\n\n# T\n\n</div>\n",
        "<div align=\"center\">\n\n## T\n\n</div>\n",
        "<div align=\"center\">\n\n# T\n\n*s*\n",
    ] {
        let Some(m) = word(md, false) else { return };
        assert!(!m.document.contains("w:val=\"Title\""), "{md:?}: {}", m.document);
    }
}

// ---------------------------------------------------------------- DOCX-008: a note without an original

// spec: DOCX-008, DOCX-009
#[test]
fn a_note_without_an_original_gets_a_reference_from_its_print_properties_and_the_import_reads_them_back() {
    let Some(pandoc) = real_pandoc() else { return };
    let dir = temp_dir("generated");
    let props = "print-paper: Letter\nprint-margins: 20 25 20 30\nprint-font: serif\nprint-font-size: 12\nprint-line-spacing: 1.15\nprint-paragraph-spacing: 8\nprint-align: justify\nprint-heading-color: \"#2F5496\"\nprint-heading-weight: bold\nprint-heading-sizes: 20 16 14 12 12 12\nprint-break-before: 1";
    let text = format!("---\n{props}\n---\n\n# Head\n\nBody text of the note.\n\n## Second\n\nMore text.\n");
    std::fs::write(dir.join("Plain.md"), &text).unwrap();
    let vault = Vault::open(&dir).unwrap();
    let job = Job::new(pandoc.clone(), &vault, Path::new("Plain.md"), &text, None, false, true);
    let bytes = export_valid(&job);
    let (document, styles) = (part(&bytes, "word/document.xml"), part(&bytes, "word/styles.xml"));
    assert!(document.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>") && document.contains("w:top=\"1134\" w:right=\"1417\" w:bottom=\"1134\" w:left=\"1701\""), "the page of the note");
    assert_eq!(attr(&styles, "rPrDefault", "rPr", "rFonts", "ascii").as_deref(), Some("Times New Roman"));
    assert_eq!(attr(&styles, "Normal", "pPr", "jc", "val").as_deref(), Some("both"));
    assert_eq!(attr(&styles, "Heading1", "rPr", "color", "val").as_deref(), Some("2F5496"));
    assert!(children(&styles, "Heading1", "pPr").contains(&"pageBreakBefore".to_string()));
    // the Word import of that file gives the properties back
    let theme = Package::read(&bytes).unwrap().text("word/theme/theme1.xml").map(String::from).ok();
    let read = docx_look::read(&document, Some(&styles), theme.as_deref());
    let got: Vec<(String, String)> = read.props().into_iter().filter(|(k, _)| k.starts_with("print-") && !["print-title", "print-toc"].contains(k)).map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string())).collect();
    let want: Vec<(String, String)> = props.lines().map(|l| l.split_once(": ").unwrap()).map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string())).collect();
    assert_eq!(got, want);
    // a note that says nothing: pandoc's default look and nothing of the page
    let bare = Job::new(pandoc.clone(), &vault, Path::new("Plain.md"), "# Head\n\nText\n", None, false, true);
    let bytes = export_valid(&bare);
    assert!(!part(&bytes, "word/document.xml").contains("<w:pgSz"), "pandoc's section as it is");
    let styles = part(&bytes, "word/styles.xml");
    assert_eq!(attr(&styles, "rPrDefault", "rPr", "rFonts", "asciiTheme").as_deref(), Some("minorHAnsi"), "the theme font of pandoc's default");
    assert_eq!(attr(&styles, "Heading1", "rPr", "color", "themeColor").as_deref(), Some("accent1"));
    assert_eq!(attr(&styles, "pPrDefault", "pPr", "spacing", "line"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- DOCX-002: the run, the failures

/// A stand-in for pandoc, a shell script with this `body` (unix only).
#[cfg(unix)]
fn fake_pandoc(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("pandoc");
    crate::test_exec::write_script(&p, &format!("#!/bin/sh\n{body}\n"));
    p
}

fn job_with(pandoc: PathBuf, root: &Path, text: &str) -> Job {
    Job { pandoc, root: root.to_path_buf(), text: text.to_string(), look: PrintLook::default(), original: None, toc: false, timeout: Duration::from_secs(30), thesis: None, warnings: vec![] }
}

// spec: DOCX-002
#[test]
fn a_pandoc_that_is_missing_is_an_error_and_so_is_an_original_that_is_no_word_file() {
    let dir = temp_dir("missing");
    let e = export(&job_with(dir.join("no-pandoc"), &dir, "x"), &AtomicBool::new(false)).unwrap_err();
    assert!(e.starts_with("pandoc could not be started"), "{e}");
    let Some(pandoc) = real_pandoc() else { return };
    std::fs::write(dir.join("fake.docx"), "this is no zip").unwrap();
    let mut job = job_with(pandoc, &dir, "x\n");
    job.original = Some(dir.join("fake.docx"));
    assert!(export(&job, &AtomicBool::new(false)).unwrap_err().contains("original Word file cannot be the template"));
    job.original = Some(dir.join("gone.docx"));
    assert!(export(&job, &AtomicBool::new(false)).unwrap_err().contains("original Word file cannot be read"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A docx that a fake pandoc hands over as its result: a part list that finishing accepts.
#[cfg(unix)]
fn fake_result(dir: &Path) -> PathBuf {
    let path = dir.join("result.docx");
    std::fs::write(&path, docx_of(&body(&sect("<w:footnotePr/>")), &styles_xml(""), &[], &[])).unwrap();
    path
}

// spec: DOCX-002
#[cfg(unix)]
#[test]
fn pandoc_runs_in_a_folder_of_its_own_with_the_arguments_and_a_failure_writes_nothing_and_says_why() {
    let dir = temp_dir("fake");
    let record = dir.join("record");
    std::fs::create_dir_all(&record).unwrap();
    let default = dir.join("default.docx");
    std::fs::write(&default, zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())])).unwrap();
    let result = fake_result(&dir);
    // records its arguments, where it runs and the reference document, then makes the file after -o
    let ok = format!(
        "if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\n\
         printf '%s\n' \"$@\" > '{r}/args'; pwd -P > '{r}/cwd'; (cd \"$(dirname \"$1\")\" && pwd -P) > '{r}/notedir'; echo 'a warning' >&2\n\
         for a in \"$@\"; do case \"$a\" in --reference-doc=*) cp \"${{a#--reference-doc=}}\" '{r}/reference.docx';; --lua-filter=*) cp \"${{a#--lua-filter=}}\" '{r}/filter.lua';; esac; done\n\
         while [ $# -gt 0 ]; do case \"$1\" in -o) (cd \"$(dirname \"$2\")\" && pwd -P) > '{r}/outdir'; basename \"$2\" > '{r}/outname'; cp '{}' \"$2\";; esac; shift; done",
        default.display(),
        result.display(),
        r = record.display()
    );
    let pandoc = fake_pandoc(&dir, &ok);
    let mut job = job_with(pandoc.clone(), &dir, "Some text\n");
    job.toc = true;
    job.look = PrintLook::of(&note_with("print-paper: A4\nprint-font: mono"));
    let made = export(&job, &AtomicBool::new(false)).unwrap();
    // the run: the argument list of DOCX-002, in a folder outside the vault that is removed afterwards
    let args: Vec<String> = std::fs::read_to_string(record.join("args")).unwrap().lines().map(String::from).collect();
    // physical (link-free) paths, recorded by the script while the folder existed: `/var` is a link to `/private/var` on macOS (#542)
    let recorded = |name: &str| PathBuf::from(std::fs::read_to_string(record.join(name)).unwrap().trim());
    let (cwd, note_dir, out_dir) = (recorded("cwd"), recorded("notedir"), recorded("outdir"));
    assert!(args[0].ends_with("/note.md") && args[1] == "-f" && args[3] == "-t" && args[4] == "docx", "{args:?}");
    assert!(args[5].starts_with("--lua-filter=") && args[6] == format!("--resource-path={}", dir.display()) && args[7].starts_with("--reference-doc="), "{args:?}");
    assert_eq!(&args[8..args.len() - 1], ["--toc", "--toc-depth=3", "-M", "shale-text-width=6.268", "-o"], "{args:?}");
    assert!(args[0].ends_with("/note.md") && args[args.len() - 1].ends_with("/out.docx"), "{args:?}");
    assert_eq!(std::fs::read_to_string(record.join("outname")).unwrap().trim(), "out.docx", "-o names out.docx");
    assert_eq!(out_dir, cwd, "-o names a file in the folder pandoc runs in");
    assert_eq!(note_dir, cwd, "the note is in the folder pandoc runs in");
    assert!(!cwd.starts_with(std::fs::canonicalize(&dir).unwrap()) && !cwd.exists(), "outside the vault, and removed: {}", cwd.display());
    assert_eq!(std::fs::read_to_string(record.join("filter.lua")).unwrap(), FILTER, "the filter is the one built in");
    let reference = Package::read(&std::fs::read(record.join("reference.docx")).unwrap()).unwrap();
    assert_eq!(attr(reference.text("word/styles.xml").unwrap(), "rPrDefault", "rPr", "rFonts", "ascii").as_deref(), Some("Courier New"), "the reference is made of the note's look");
    // the file is finished: the page of the note is in it
    assert!(part(&made, "word/document.xml").contains("<w:pgSz w:w=\"11906\" w:h=\"16838\"/>"));
    // a failure: its first line says why, nothing is made
    let failing = fake_pandoc(
        &dir,
        &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\npwd -P > '{}/cwd2'\necho >&2\necho 'pandoc: the first line' >&2\necho 'second' >&2\nexit 3", default.display(), record.display()),
    );
    let e = export(&job_with(failing, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err();
    assert_eq!(e, "pandoc could not make the Word file: pandoc: the first line");
    assert!(!PathBuf::from(std::fs::read_to_string(record.join("cwd2")).unwrap().trim()).exists(), "removed after a failure too");
    // without a message
    let silent = fake_pandoc(&dir, &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nexit 4", default.display()));
    assert_eq!(export(&job_with(silent, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err(), "pandoc could not make the Word file: it stopped without a message");
    // a pandoc that ends well and makes no file, or one that is no Word file
    let nothing = fake_pandoc(&dir, &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nexit 0", default.display()));
    assert!(export(&job_with(nothing, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err().starts_with("pandoc made no file"));
    let garbage = fake_pandoc(&dir, &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) echo garbage > \"$2\";; esac; shift; done", default.display()));
    assert!(export(&job_with(garbage, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err().contains("cannot be finished"));
    // no default reference document
    let no_default = fake_pandoc(&dir, "echo 'no such data file' >&2; exit 1");
    assert_eq!(export(&job_with(no_default, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err(), "pandoc has no default reference document: no such data file");
    let no_message = fake_pandoc(&dir, "exit 1");
    assert!(export(&job_with(no_message, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err().ends_with("it stopped without a message"));
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: DOCX-012, IMP-062
#[cfg(unix)]
#[test]
fn a_pandoc_that_needs_more_than_the_memory_limit_has_the_limit_and_the_export_says_so_and_leaves_nothing() {
    let dir = temp_dir("heap");
    let record = dir.join("record");
    std::fs::create_dir_all(&record).unwrap();
    let default = dir.join("default.docx");
    std::fs::write(&default, zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())])).unwrap();
    // both starts (the default reference document and the conversion) record the runtime options; the conversion writes
    // half a file and dies like a GHC program whose heap is full
    let body = format!(
        "echo \"$GHCRTS\" >> '{r}/ghcrts'\nif [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\npwd -P > '{r}/cwd'\n\
         while [ $# -gt 0 ]; do case \"$1\" in -o) echo half > \"$2\";; esac; shift; done\n\
         echo 'pandoc: Heap exhausted;' >&2; echo 'pandoc: Current maximum heap size is 2147483648 bytes (2048 MB).' >&2; exit 251",
        default.display(),
        r = record.display()
    );
    let pandoc = fake_pandoc(&dir, &body);
    let e = export(&job_with(pandoc, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err();
    assert_eq!(e, "pandoc needed more than 2 GB of memory and was stopped");
    let starts: Vec<String> = std::fs::read_to_string(record.join("ghcrts")).unwrap().lines().map(String::from).collect();
    assert!(starts.len() == 2 && starts.iter().all(|s| s.ends_with("-M2g")), "the limit on every start: {starts:?}");
    let cwd = PathBuf::from(std::fs::read_to_string(record.join("cwd")).unwrap().trim());
    assert!(!cwd.exists(), "the temporary folder with the half file is removed: {}", cwd.display());
    // the default reference document too
    let heavy_default = fake_pandoc(&dir, "echo 'pandoc: Heap exhausted;' >&2; exit 251");
    assert_eq!(export(&job_with(heavy_default, &dir, "x\n"), &AtomicBool::new(false)).unwrap_err(), "pandoc needed more than 2 GB of memory and was stopped");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: DOCX-002
#[cfg(unix)]
#[test]
fn pandoc_is_killed_after_the_time_and_when_the_export_is_cancelled() {
    let dir = temp_dir("slow");
    let default = dir.join("default.docx");
    std::fs::write(&default, zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())])).unwrap();
    let slow = fake_pandoc(&dir, &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nsleep 30", default.display()));
    let mut job = job_with(slow.clone(), &dir, "x\n");
    job.timeout = Duration::from_millis(1000);
    let started = std::time::Instant::now();
    assert_eq!(export(&job, &AtomicBool::new(false)).unwrap_err(), "pandoc needed more than 1 seconds and was stopped");
    assert!(started.elapsed() < Duration::from_secs(20));
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    assert_eq!(export(&job_with(slow, &dir, "x\n"), &cancel).unwrap_err(), "stopped");
    stopper.join().unwrap();
    // the default reference too can be stopped
    let hang = fake_pandoc(&dir, "sleep 30");
    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(export(&job_with(hang, &dir, "x\n"), &cancel).unwrap_err(), "stopped");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- DOCX-001: the dialog

mod dialog_tests {
    use super::super::dialog::{footer, show, DialogResult, DocxDialog};
    use super::*;
    use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    struct H {
        ctx: egui::Context,
        t: f64,
        last: DialogResult,
    }

    impl H {
        fn new(d: &mut DocxDialog) -> H {
            let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
            h.frame(d, vec![]);
            h.frame(d, vec![]);
            h
        }
        fn frame(&mut self, d: &mut DocxDialog, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 900.0))), time: Some(self.t), events, ..Default::default() };
            let mut res = DialogResult::Open;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, d);
                    let r = footer(ui, d);
                    if r != DialogResult::Open {
                        res = r;
                    }
                });
            });
            out.textures_delta.clear();
            if res != DialogResult::Open {
                self.last = res;
            }
        }
        fn click(&mut self, d: &mut DocxDialog, key: &str) {
            let r = d.rects.iter().find(|(k, _)| k == key).map(|r| r.1).unwrap_or_else(|| panic!("no control {key}: {:?}", d.rects.iter().map(|r| &r.0).collect::<Vec<_>>()));
            let p = r.center();
            let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(d, vec![Event::PointerMoved(p)]);
            self.frame(d, vec![b(true)]);
            self.frame(d, vec![b(false)]);
            self.frame(d, vec![]);
        }
    }

    fn has(d: &DocxDialog, key: &str) -> bool {
        d.rects.iter().any(|r| r.0 == key)
    }

    fn dialog(pandoc: Option<&str>, original: Option<&str>) -> DocxDialog {
        DocxDialog::new(PathBuf::from("Notes/Report.md"), "Report".into(), PathBuf::from("/v/exports/Report.docx"), pandoc.map(PathBuf::from), original.map(PathBuf::from), true, true)
    }

    // spec: DOCX-001
    #[test]
    fn the_dialog_has_the_save_path_the_table_of_contents_open_after_and_export_and_cancel() {
        let mut d = dialog(Some("/usr/bin/pandoc"), None);
        let mut h = H::new(&mut d);
        assert_eq!(d.path, "/v/exports/Report.docx");
        for key in ["path", "toc", "open-after", "export", "cancel", "pandoc"] {
            assert!(has(&d, key), "{key}");
        }
        assert!(!has(&d, "use-original") && !has(&d, "working"), "the box for the original only with an original");
        assert!(d.toc && !d.open_after && !d.use_original);
        h.click(&mut d, "toc");
        h.click(&mut d, "open-after");
        assert!(!d.toc && d.open_after);
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Export);
        h.click(&mut d, "cancel");
        assert_eq!(h.last, DialogResult::Cancel);
    }

    // spec: DOCX-013
    #[test]
    fn the_dialog_has_a_box_for_the_page_break_after_the_table_of_contents_which_needs_the_contents() {
        let mut d = dialog(Some("/usr/bin/pandoc"), None);
        let mut h = H::new(&mut d);
        assert!(has(&d, "toc-break") && d.toc_break, "on by default");
        h.click(&mut d, "toc-break");
        assert!(!d.toc_break);
        h.click(&mut d, "toc-break");
        assert!(d.toc_break);
        // with the contents off the box is greyed and a click does nothing
        h.click(&mut d, "toc");
        h.click(&mut d, "toc-break");
        assert!(!d.toc && d.toc_break);
    }

    // spec: DOCX-001
    #[test]
    fn the_box_for_the_original_is_there_with_an_original_and_ticked() {
        let mut d = dialog(Some("/usr/bin/pandoc"), Some("z_media/Report.docx"));
        let mut h = H::new(&mut d);
        assert!(has(&d, "use-original") && d.use_original, "ticked");
        h.click(&mut d, "use-original");
        assert!(!d.use_original);
        h.click(&mut d, "use-original");
        assert!(d.use_original);
    }

    // spec: DOCX-001
    #[test]
    fn without_pandoc_the_dialog_says_so_and_export_is_disabled() {
        let mut d = dialog(None, None);
        let mut h = H::new(&mut d);
        assert!(has(&d, "pandoc"));
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Open, "Export does nothing");
        h.click(&mut d, "cancel");
        assert_eq!(h.last, DialogResult::Cancel);
        // the line says where pandoc was found
        let mut found = dialog(Some("/opt/pandoc/bin/pandoc"), None);
        let _ = H::new(&mut found);
        assert_eq!(found.pandoc.as_deref(), Some(Path::new("/opt/pandoc/bin/pandoc")));
    }

    // spec: DOCX-002, JOB-010, JOB-019
    #[cfg(unix)]
    #[test]
    fn the_export_runs_as_a_task_the_dialog_shows_it_and_cancel_and_closing_stop_it() {
        use crate::busy::Busy;
        use dialog::DOCX_EXPORT;
        let wait_idle = |busy: &Busy| {
            let start = std::time::Instant::now();
            while busy.is_running(&DOCX_EXPORT) {
                assert!(start.elapsed() < Duration::from_secs(30), "the task ends with its job");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let dir = temp_dir("dialog-job");
        let default = dir.join("default.docx");
        std::fs::write(&default, zip_of(&[("[Content_Types].xml", "<t/>"), ("word/styles.xml", &pandoc_styles())])).unwrap();
        // every fake pandoc in a folder of its own: the cancelled one may still run for a moment, and a file that runs cannot be
        // written again (ETXTBSY, #549)
        let sub = |name: &str| {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let slow = fake_pandoc(&sub("slow"), &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nsleep 30", default.display()));
        let busy = Busy::default();
        let mut d = dialog(Some("/usr/bin/pandoc"), None).with_busy(&busy);
        let mut h = H::new(&mut d);
        assert!(d.poll().is_none() && !d.working() && busy.list().is_empty());
        assert!(d.start(job_with(slow.clone(), &dir, "x\n"), || {}));
        h.frame(&mut d, vec![]);
        assert!(d.working() && has(&d, "working"), "the dialog shows that it works");
        let listed = busy.info(&DOCX_EXPORT);
        assert!(listed.as_ref().is_some_and(|t| t.cancellable && t.label == "Exporting “Report” to Word…"), "the task is listed with its text and Cancel: {listed:?}");
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Open, "no second export beside the running one");
        let other = dialog(Some("/usr/bin/pandoc"), None).with_busy(&busy);
        assert!(!other.can_export(), "another dialog waits too (JOB-005)");
        // Cancel stops the work (JOB-010): the task leaves the list when pandoc is gone
        d.cancel();
        assert!(!d.working() && d.poll().is_none());
        wait_idle(&busy);
        assert!(busy.list().is_empty() && other.can_export());
        // a job that has ended hands its result over once
        let nothing = fake_pandoc(&sub("nothing"), "exit 7");
        assert!(d.start(job_with(nothing, &dir, "x\n"), || {}));
        let start = std::time::Instant::now();
        let got = loop {
            if let Some(g) = d.poll() {
                break g;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "the job ends");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(got.unwrap_err().contains("pandoc has no default reference document") && d.poll().is_none() && !d.working());
        // dropping the dialog stops pandoc
        let marker = dir.join("still-running");
        let waiting = fake_pandoc(&sub("waiting"), &format!("if [ \"$1\" = \"--print-default-data-file\" ]; then cat '{}'; exit 0; fi\nsleep 3\ntouch '{}'", default.display(), marker.display()));
        assert!(d.start(job_with(waiting, &dir, "x\n"), || {}));
        std::thread::sleep(Duration::from_millis(500));
        drop(d);
        wait_idle(&busy);
        std::thread::sleep(Duration::from_millis(3500));
        assert!(!marker.exists(), "pandoc was killed with the dialog, and its task is gone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------- a developer tool

// A developer tool, not a test of a rule (DEV-004): `scripts/roundtrip-docx.sh` runs this ignored test, which imports the
// Word file `SHALE_ROUNDTRIP_DOCX` into a new vault with the real pandoc, exports the note, writes the Word file to
// `SHALE_ROUNDTRIP_OUT` (the note's text beside it, `.md`) and prints the share of the words that came back. A step that fails prints `import error:` or
// `export error:` with the message and fails the test (scripts/docx-corpus.sh reads these lines).
// spec: DOCX-010, DEV-037
#[test]
#[ignore = "run by scripts/roundtrip-docx.sh and scripts/docx-corpus.sh"]
fn dev_004_roundtrip_imports_and_exports_a_word_file_and_prints_the_share_of_words_that_came_back() {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| panic!("{name} is not set"));
    let (input, out) = (var("SHALE_ROUNDTRIP_DOCX"), var("SHALE_ROUNDTRIP_OUT"));
    let pandoc = docx::locate("").expect("pandoc is not installed");
    let trip = match try_round_trip("roundtrip", &input).expect("pandoc is not installed") {
        Ok(trip) => trip,
        Err(e) => {
            let (step, message) = e.split_once(": ").unwrap_or(("", &e));
            let message = message.lines().next().unwrap_or_default();
            println!("{step} error: {message}");
            panic!("{step} error: {message}");
        }
    };
    std::fs::write(&out, &trip.bytes).unwrap();
    let note = out.with_extension("md");
    std::fs::write(&note, &trip.text).unwrap();
    let (before, after) = (words_of(&pandoc, &input), words_of(&pandoc, &out));
    let common = (share(&before, &after) * before.len() as f64).round() as usize;
    if before.is_empty() {
        println!("words that came back: 0 of 0 (no words); the export has {}", after.len());
    } else {
        println!("words that came back: {common} of {} ({:.1} %); the export has {}", before.len(), share(&before, &after) * 100.0, after.len());
    }
    println!("original: {}", input.display());
    println!("note: {}", note.display());
    println!("exported: {}", out.display());
}
