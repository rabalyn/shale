//! The fonts of the built-in PDF (PDF-036, PDF-037, PDF-038, BOOK-022, #658): a family of the
//! vault or of a "system" folder of the test (never the machine's fonts) is looked up, embedded
//! with its real faces, and what could not be used is a warning. The test fonts have the outlines
//! of Liberation Serif (SIL OFL 1.1, `assets/fonts/`) under other names, weights and slants.

use crate::fonts::resolve::Values;
use crate::fonts::scan::{scan_system, scan_vault, Catalogue};
use crate::fonts::test_fonts::{folder, Spec};
use crate::pdf::export::tests::{contents, hex};
use crate::pdf::export::{export_note_until, ExportOptions};
use crate::pdf::export_fonts::{FontsAsk, Made};
use crate::vault::Vault;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const BASE: &[u8] = include_bytes!("../../assets/fonts/LiberationSerif-Regular.ttf");

/// The four faces of a family: regular, bold, italic, bold italic (weight, slanted).
pub(super) const FOUR: [(u16, bool); 4] = [(400, false), (700, false), (400, true), (700, true)];

/// Write the faces of a test family below `dir`; the files are named after family, weight and slant.
pub(super) fn family(dir: &Path, name: &'static str, faces: &[(u16, bool)]) {
    for (weight, italic) in faces {
        Spec::new(name, *weight, *italic).write_on(BASE, dir, &format!("{}-{weight}{}.ttf", name.replace(' ', ""), if *italic { "It" } else { "" }));
    }
}

/// A vault with a `fonts/` folder, and a folder that stands for the installed fonts.
pub(super) struct Env {
    pub root: PathBuf,
    pub system: PathBuf,
}

impl Env {
    pub fn new(tag: &str) -> Env {
        let base = folder(&format!("pdf-{tag}"));
        let (root, system) = (base.join("vault"), base.join("system"));
        std::fs::create_dir_all(root.join("fonts")).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        Env { root, system }
    }

    /// The font scan as it would be: the vault's fonts and the system folder's.
    pub fn ask(&self, settings: Values) -> Arc<FontsAsk> {
        let catalogue = Catalogue { vault: Arc::new(scan_vault(&self.root)), system: Arc::new(scan_system(Some(std::slice::from_ref(&self.system)))), system_ready: true, generation: 1 };
        Arc::new(FontsAsk { catalogue, vault: settings, late: false })
    }

    /// Plain options (no title, contents or page numbers) that look the fonts up.
    pub fn opts(&self, settings: Values) -> ExportOptions {
        ExportOptions { title: false, toc: false, page_numbers: false, fonts: Some(self.ask(settings)), ..ExportOptions::for_note() }
    }

    /// Export a note with the options.
    pub fn export(&self, text: &str, opts: &ExportOptions) -> Made {
        let vault = Vault::open(&self.root).unwrap();
        export_note_until(&vault, Path::new("Note.md"), text, opts, None).unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap());
    }
}

/// The `BaseFont` of every font dictionary of the PDF.
pub(super) fn base_fonts(pdf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf);
    text.split("/BaseFont /").skip(1).map(|r| r.chars().take_while(|c| !c.is_whitespace() && *c != '/').collect()).collect()
}

/// The `BaseFont` of the font that the PDF draws a piece of text in: the text must be drawn as one run.
pub(super) fn font_of(pdf: &[u8], text: &str) -> String {
    let c = contents(pdf);
    let at = c.find(&format!("<{}", hex(text))).unwrap_or_else(|| panic!("{text:?} is drawn"));
    let op = c[..at].rfind("q BT /").unwrap();
    let res: String = c[op + 6..].chars().take_while(|c| !c.is_whitespace()).collect();
    let all = String::from_utf8_lossy(pdf).into_owned();
    let n: String = all.split(&format!("/{res} ")).nth(1).unwrap_or_else(|| panic!("the resource {res} is listed")).chars().take_while(char::is_ascii_digit).collect();
    let object = all.split(&format!("\n{n} 0 obj\n")).nth(1).expect("the font object");
    object.split("/BaseFont /").nth(1).expect("a font dictionary").chars().take_while(|c| !c.is_whitespace()).collect()
}

fn names_font(base_font: &str, ps_name: &str) -> bool {
    base_font == ps_name || base_font.strip_suffix(ps_name).is_some_and(|p| p.len() == 7 && p.ends_with('+'))
}

// spec: PDF-036
#[test]
fn pdf_036_a_family_with_four_faces_embeds_them_and_no_imitation_is_drawn() {
    let e = Env::new("four");
    family(&e.root.join("fonts"), "Test Serif", &FOUR);
    let made = e.export("---\nprint-font: Test Serif\n---\nZplain **Zbold** *Zslant* ***Zboth*** arrow → end\n", &e.opts(Values::default()));
    assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    let names = base_fonts(&made.bytes);
    for ps in ["TestSerif-400", "TestSerif-700", "TestSerif-400It", "TestSerif-700It"] {
        assert!(names.iter().any(|n| names_font(n, ps)), "{ps} is embedded: {names:?}");
    }
    for (word, ps) in [("Zplain", "TestSerif-400"), ("Zbold", "TestSerif-700"), ("Zslant", "TestSerif-400It"), ("Zboth", "TestSerif-700It")] {
        assert!(names_font(&font_of(&made.bytes, word), ps), "{word} is set in {ps}");
    }
    let c = contents(&made.bytes);
    assert!(!c.contains("2 Tr") && !c.contains("1 0 0.2 1"), "no stroke and no shear is drawn for a real face");
    // the arrow is not in the font: it is drawn from the bundled fonts as outlines (PDF-010)
    assert!(c.contains(" f Q"), "the arrow is an outline");
    // the faces are cut down to the characters used: four fonts of 400 kB are not in the file
    assert!(made.bytes.len() < 200_000, "{} bytes", made.bytes.len());
    // and what is left of them draws: a reader finds the glyphs of the cut-down fonts
    let ink = |bytes: &[u8]| crate::pdf::export::tests::render(bytes)[0].2.chunks(4).filter(|px| px[0] < 200).count();
    let plain = e.export("Zplain Zbold Zslant Zboth arrow → end\n", &ExportOptions { fonts: None, ..e.opts(Values::default()) });
    assert!(ink(&made.bytes) * 2 > ink(&plain.bytes) && ink(&plain.bytes) > 50, "{} against {}", ink(&made.bytes), ink(&plain.bytes));
}

// spec: PDF-036
#[test]
fn pdf_036_a_family_without_bold_warns_once_and_imitates_it_and_the_faces_it_has_are_used() {
    let e = Env::new("nobold");
    family(&e.system, "Test Sys", &[(400, false), (400, true)]);
    let made = e.export("---\nprint-font: Test Sys\n---\nZone **Zbold** and **Zbold2** and *Zslant*\n\n# Head\n", &e.opts(Values::default()));
    assert_eq!(made.warnings, vec!["Test Sys has no bold face: bold text is imitated.".to_string()], "once, though bold is used three times");
    let c = contents(&made.bytes);
    assert!(c.contains("2 Tr"), "bold is drawn with a stroke");
    assert!(names_font(&font_of(&made.bytes, "Zbold"), "TestSys-400"), "the regular face is drawn thicker");
    assert!(names_font(&font_of(&made.bytes, "Zslant"), "TestSys-400It"), "the italic face is the family's own");
    assert!(!c.contains("1 0 0.2 1"), "italic is not sheared");
}

// spec: PDF-036
#[test]
fn pdf_036_headings_follow_the_text_font_unless_a_sans_font_is_set_and_code_has_its_own() {
    let e = Env::new("roles");
    let fonts = e.root.join("fonts");
    family(&fonts, "Test Serif", &FOUR);
    family(&fonts, "Test Sans", &[(400, false), (700, false)]);
    family(&fonts, "Test Mono", &[(400, false)]);
    let body = "# Zhead\n\nZtext\n\n```\nZcode\n```\n\nand `Zinline`\n";
    let follow = e.export(&format!("---\nprint-font: Test Serif\nprint-font-mono: Test Mono\n---\n{body}"), &e.opts(Values::default()));
    assert!(names_font(&font_of(&follow.bytes, "Zhead"), "TestSerif-700"), "headings are in the bold of the text font");
    assert!(names_font(&font_of(&follow.bytes, "Ztext"), "TestSerif-400"));
    assert!(names_font(&font_of(&follow.bytes, "Zcode"), "TestMono-400"), "code blocks");
    assert!(names_font(&font_of(&follow.bytes, "Zinline"), "TestMono-400"), "inline code");
    let sans = e.export(&format!("---\nprint-font: Test Serif\nprint-font-sans: Test Sans\n---\n{body}"), &e.opts(Values::default()));
    assert!(names_font(&font_of(&sans.bytes, "Zhead"), "TestSans-700"), "headings in the sans font, in its own bold");
    assert!(names_font(&font_of(&sans.bytes, "Ztext"), "TestSerif-400"));
    assert!(names_font(&font_of(&sans.bytes, "Zcode"), "Hack"), "code stays Hack without print-font-mono");
    assert!(sans.warnings.is_empty(), "{:?}", sans.warnings);
    // the title, contents and page numbers are set in the headings' font too
    let all = e.export(&format!("---\nprint-font-sans: Test Sans\n---\n{body}"), &ExportOptions { title: true, toc: true, page_numbers: true, ..e.opts(Values::default()) });
    assert!(names_font(&font_of(&all.bytes, "1 / "), "TestSans-400"), "the page numbers");
    assert!(names_font(&font_of(&all.bytes, "Contents"), "TestSans-700"), "the contents");
}

// spec: PDF-036, PDF-038
#[test]
fn pdf_036_a_font_whose_licence_says_no_subsetting_is_embedded_whole_and_the_others_are_cut_down() {
    let e = Env::new("whole");
    Spec::new("Test Whole", 400, false).licence(0x0100, 4).write_on(BASE, &e.root.join("fonts"), "TestWhole.ttf");
    family(&e.root.join("fonts"), "Test Cut", &[(400, false)]);
    let whole = e.export("---\nprint-font: Test Whole\n---\nZtext\n", &e.opts(Values::default()));
    let cut = e.export("---\nprint-font: Test Cut\n---\nZtext\n", &e.opts(Values::default()));
    assert!(base_fonts(&whole.bytes).contains(&"TestWhole-400".to_string()), "a whole font has no subset tag: {:?}", base_fonts(&whole.bytes));
    assert!(base_fonts(&cut.bytes).iter().any(|n| names_font(n, "TestCut-400") && n.contains('+')), "{:?}", base_fonts(&cut.bytes));
    assert!(whole.bytes.len() > cut.bytes.len() + 50_000, "{} against {}", whole.bytes.len(), cut.bytes.len());
}

// spec: PDF-037
#[test]
fn pdf_037_a_missing_font_and_a_file_that_is_no_font_each_warn_and_fall_through_and_the_pdf_is_made() {
    let e = Env::new("missing");
    family(&e.root.join("fonts"), "Test Serif", &[(400, false)]);
    std::fs::write(e.root.join("fonts/junk.ttf"), b"not a font").unwrap();
    // the note's font is missing: the vault's setting is next
    let settings = Values { text: Some("Test Serif".into()), ..Values::default() };
    let next = e.export("---\nprint-font: Garamond Nowhere\n---\nZtext\n", &e.opts(settings));
    assert_eq!(next.warnings.len(), 1, "{:?}", next.warnings);
    assert!(next.warnings[0].contains("Garamond Nowhere") && next.warnings[0].contains("was not found") && next.warnings[0].contains("`print-font`"), "{:?}", next.warnings);
    assert!(names_font(&font_of(&next.bytes, "Ztext"), "TestSerif-400"), "the next entry is used");
    // nothing else gives a font: the default, named in the warning
    let default = e.export("---\nprint-font: Garamond Nowhere\n---\nZtext\n", &e.opts(Values::default()));
    assert!(default.warnings[0].ends_with("using Ubuntu Light."), "{:?}", default.warnings);
    assert!(names_font(&font_of(&default.bytes, "Ztext"), "UbuntuLight") && default.bytes.starts_with(b"%PDF"));
    // a file that is no font
    let junk = e.export("---\nprint-font: fonts/junk.ttf\nprint-font-mono: Mono Nowhere\n---\nZtext\n", &e.opts(Values::default()));
    assert_eq!(junk.warnings.len(), 2, "{:?}", junk.warnings);
    assert!(junk.warnings.iter().any(|w| w.contains("junk.ttf") && w.contains("cannot be read")), "{:?}", junk.warnings);
    assert!(junk.warnings.iter().any(|w| w.contains("`print-font-mono`") && w.contains("Mono Nowhere")), "{:?}", junk.warnings);
    assert!(junk.bytes.starts_with(b"%PDF"));
}

// spec: PDF-037, PDF-038
#[test]
fn pdf_037_a_font_whose_licence_refuses_embedding_is_a_warning_and_is_not_in_the_pdf() {
    let e = Env::new("refused");
    Spec::new("Test Locked", 400, false).licence(0x0002, 4).write_on(BASE, &e.system, "TestLocked.ttf");
    let made = e.export("---\nprint-font: Test Locked\n---\nZtext\n", &e.opts(Values::default()));
    assert_eq!(made.warnings.len(), 1, "{:?}", made.warnings);
    assert!(made.warnings[0].contains("Test Locked") && made.warnings[0].contains("licence") && made.warnings[0].ends_with("using Ubuntu Light."), "{:?}", made.warnings);
    assert!(!String::from_utf8_lossy(&made.bytes).contains("TestLocked"), "the font is not embedded");
    assert!(names_font(&font_of(&made.bytes, "Ztext"), "UbuntuLight"));
}

// spec: PDF-037
#[test]
fn pdf_037_an_export_without_a_scan_looks_nothing_up_and_one_with_the_scan_not_in_reports_it_when_late() {
    let e = Env::new("late");
    family(&e.system, "Test Sys", &[(400, false)]);
    let mut ask = (*e.ask(Values::default())).clone();
    ask.catalogue.system_ready = false;
    ask.catalogue.system = Arc::default();
    ask.late = true;
    let note = "---\nprint-font: Test Sys\n---\nZtext\n";
    let late = e.export(note, &ExportOptions { fonts: Some(Arc::new(ask)), ..e.opts(Values::default()) });
    assert_eq!(late.warnings.len(), 1, "{:?}", late.warnings);
    assert!(late.warnings[0].contains("not looked up in time"), "{:?}", late.warnings);
    assert!(names_font(&font_of(&late.bytes, "Ztext"), "UbuntuLight"), "without the system fonts the default is used");
    let none = e.export(note, &ExportOptions { fonts: None, ..e.opts(Values::default()) });
    assert!(none.warnings.is_empty() && names_font(&font_of(&none.bytes, "Ztext"), "UbuntuLight"), "an export that was not given the scan sets only sans, serif and mono");
}

// spec: PDF-036
#[test]
fn pdf_036_a_missing_bold_italic_uses_the_nearest_face_and_imitates_the_rest_and_says_so_once() {
    let e = Env::new("nearest");
    family(&e.root.join("fonts"), "Test Heavy", &[(400, false), (700, false)]);
    family(&e.root.join("fonts"), "Test Slant", &[(400, false), (400, true)]);
    // bold and italic from a family with a bold face only: the bold face, slanted
    let heavy = e.export("---\nprint-font: Test Heavy\n---\n***Zboth*** and *Zslant*\n", &e.opts(Values::default()));
    assert!(names_font(&font_of(&heavy.bytes, "Zboth"), "TestHeavy-700"));
    assert_eq!(heavy.warnings, vec!["Test Heavy has no italic face: italic text is imitated.".to_string(), "Test Heavy has no bold italic face: bold italic text is imitated.".to_string()]);
    assert!(contents(&heavy.bytes).contains("1 0 0.2 1"), "the missing slant is sheared");
    // from one with an italic face only: the italic face, thickened
    let slant = e.export("---\nprint-font: Test Slant\n---\n***Zboth***\n", &e.opts(Values::default()));
    assert!(names_font(&font_of(&slant.bytes, "Zboth"), "TestSlant-400It"));
    assert_eq!(slant.warnings, vec!["Test Slant has no bold italic face: bold italic text is imitated.".to_string()]);
    assert!(contents(&slant.bytes).contains("2 Tr") && !contents(&slant.bytes).contains("1 0 0.2 1"));
}

// spec: PDF-036, PDF-037
#[test]
fn pdf_036_a_font_with_outlines_that_are_not_truetype_is_drawn_as_outlines_and_a_file_that_went_bad_is_a_warning() {
    let e = Env::new("cff");
    std::fs::copy("assets/fonts/Virgil.otf", e.root.join("fonts/Chosen.otf")).unwrap();
    let made = e.export("---\nprint-font: fonts/Chosen.otf\n---\nZtext\n", &e.opts(Values::default()));
    assert!(made.warnings.is_empty(), "{:?}", made.warnings);
    assert!(!base_fonts(&made.bytes).iter().any(|n| n.contains("Virgil")), "no text font is embedded for it");
    assert!(contents(&made.bytes).contains(" f Q"), "the characters are outlines");
    // a font that was found by the scan and cannot be read when the export starts
    family(&e.root.join("fonts"), "Test Gone", &[(400, false)]);
    let opts = e.opts(Values::default());
    std::fs::write(e.root.join("fonts/TestGone-400.ttf"), b"changed").unwrap();
    let gone = e.export("---\nprint-font: Test Gone\n---\nZtext\n", &opts);
    assert_eq!(gone.warnings.len(), 1, "{:?}", gone.warnings);
    assert!(gone.warnings[0].contains("Test Gone") && gone.warnings[0].ends_with("using Ubuntu Light."), "{:?}", gone.warnings);
    assert!(names_font(&font_of(&gone.bytes, "Ztext"), "UbuntuLight"));
}
