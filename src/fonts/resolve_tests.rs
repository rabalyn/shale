//! The font resolver (PDF-034, PDF-035, PDF-037, PDF-038, CFG-080, ADR-0040): the order of the
//! entries, the sources, the name matching, the faces, the licences and the missing fonts. The
//! fonts are tiny test fonts in a test vault and a test "system" folder.

use super::resolve::{resolve, Chosen, Engine, Entry, FaceKind, Family, Request, Resolved, Role, Values};
use super::scan::{scan_system, scan_vault, Catalogue, Place};
use super::test_fonts::{folder, Spec};
use crate::pdf::write::FontKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Env {
    root: PathBuf,
    sys: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Env {
        Env { root: folder(&format!("{tag}-vault")), sys: folder(&format!("{tag}-sys")) }
    }

    fn vault(&self, spec: Spec, file: &str) -> PathBuf {
        spec.write(&self.root, file)
    }

    fn system(&self, spec: Spec, file: &str) {
        spec.write(&self.sys, file);
    }

    fn catalogue(&self, system_ready: bool) -> Catalogue {
        Catalogue { vault: Arc::new(scan_vault(&self.root)), system: Arc::new(if system_ready { scan_system(Some(std::slice::from_ref(&self.sys))) } else { Vec::new() }), system_ready, generation: 0 }
    }

    fn run(&self, req: &Request) -> Resolved {
        resolve(req, &self.catalogue(true))
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.sys);
    }
}

fn request(root: &Path, engine: Engine) -> Request<'_> {
    Request { engine, vault_root: root, note: Values::default(), book: false, chapters: vec![], set: None, vault: Values::default() }
}

fn text(v: &str) -> Values {
    Values { text: Some(v.into()), ..Values::default() }
}

fn family(r: &Resolved, role: Role) -> &Family {
    match &r.pick(role).chosen {
        Chosen::Family(f) => f,
        other => panic!("{role:?} is {other:?}"),
    }
}

fn file_names(f: &Family) -> Vec<Option<String>> {
    f.faces.iter().map(|x| x.as_ref().map(|x| x.path.file_name().unwrap().to_string_lossy().into_owned())).collect()
}

fn garamond(file: impl Fn(&str) -> PathBuf) -> [PathBuf; 4] {
    [file("Garamond-Regular.ttf"), file("Garamond-Bold.ttf"), file("Garamond-Italic.ttf"), file("Garamond-BoldItalic.ttf")]
}

fn with_garamond(env: &Env, in_vault: bool) {
    for (file, weight, italic) in [("Garamond-Regular.ttf", 400, false), ("Garamond-Bold.ttf", 700, false), ("Garamond-Italic.ttf", 400, true), ("Garamond-BoldItalic.ttf", 700, true)] {
        let spec = Spec::new("Garamond", weight, italic);
        if in_vault {
            env.vault(spec, &format!("fonts/{file}"));
        } else {
            env.system(spec, file);
        }
    }
}

// spec: PDF-034
#[test]
fn pdf_034_the_note_wins_then_the_set_then_the_vault_then_the_default() {
    let env = Env::new("order");
    env.vault(Spec::new("Alpha", 400, false), "fonts/Alpha.ttf");
    env.vault(Spec::new("Bravo", 400, false), "fonts/Bravo.ttf");
    env.system(Spec::new("Charlie", 400, false), "Charlie.ttf");
    env.system(Spec::new("Delta", 400, false), "Delta.ttf");
    let name = |r: &Resolved| (r.text.entry, family(r, Role::Text).name.clone());

    let mut req = request(&env.root, Engine::Tex);
    req.note = text("Alpha");
    req.set = Some(("uni".into(), text("Charlie")));
    req.vault = text("Bravo");
    assert_eq!(name(&env.run(&req)), (Entry::Note, "Alpha".into()));
    req.note = Values::default();
    assert_eq!(name(&env.run(&req)), (Entry::Set, "Charlie".into()), "the set comes after the note and before the vault");
    req.set = None;
    assert_eq!(name(&env.run(&req)), (Entry::Vault, "Bravo".into()));
    req.vault = Values::default();
    let r = env.run(&req);
    assert_eq!((r.text.entry, &r.text.chosen), (Entry::Default, &Chosen::Default("the template's font")));

    // the built-in PDF has no set
    let mut req = request(&env.root, Engine::Pdf);
    req.set = Some(("uni".into(), text("Charlie")));
    req.vault = text("Delta");
    assert_eq!(name(&env.run(&req)), (Entry::Vault, "Delta".into()));
    req.vault = Values::default();
    let r = env.run(&req);
    assert_eq!(r.text.chosen, Chosen::Default("Ubuntu Light"));
    assert_eq!((r.sans.chosen.clone(), r.mono.chosen.clone()), (Chosen::Default("the text font"), Chosen::Default("Hack")));
    assert!(r.warnings.is_empty() && !r.pending);
}

// spec: PDF-034
#[test]
fn pdf_034_each_key_is_decided_on_its_own() {
    let env = Env::new("own");
    env.vault(Spec::new("Alpha", 400, false), "fonts/Alpha.ttf");
    env.vault(Spec::new("Bravo", 400, false), "fonts/Bravo.ttf");
    env.system(Spec::new("Delta", 400, false), "Delta.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = Values { sans: Some("Alpha".into()), ..Values::default() };
    req.vault = Values { text: Some("Bravo".into()), sans: Some("Bravo".into()), mono: Some("Delta".into()) };
    let r = env.run(&req);
    assert_eq!((r.text.entry, family(&r, Role::Text).name.as_str()), (Entry::Vault, "Bravo"));
    assert_eq!((r.sans.entry, family(&r, Role::Sans).name.as_str()), (Entry::Note, "Alpha"));
    assert_eq!((r.mono.entry, family(&r, Role::Mono).name.as_str(), family(&r, Role::Mono).place.clone()), (Entry::Vault, "Delta", Place::System));
}

// spec: PDF-034
#[test]
fn pdf_034_the_properties_of_a_note_and_the_settings_of_the_vault_are_read() {
    let v = Values::of_note("---\nprint-font: Garamond\nprint-font-sans: \"Fira Sans\"\nprint-font-mono:\ntitle: x\n---\nBody\n");
    assert_eq!(v, Values { text: Some("Garamond".into()), sans: Some("Fira Sans".into()), mono: None }, "an empty value is a missing one");
    assert_eq!(Values::of_note("no front matter"), Values::default());
    let settings = crate::vault_state::VaultSettings { print_font: " Libertinus Serif ".into(), print_font_mono: "Hack".into(), ..Default::default() };
    assert_eq!(Values::of_settings(&settings), Values { text: Some("Libertinus Serif".into()), sans: None, mono: Some("Hack".into()) });
}

// spec: PDF-034
#[test]
fn pdf_034_the_words_sans_serif_and_mono_keep_their_meaning_for_the_text_only() {
    let env = Env::new("words");
    for (word, kind, shown) in [("sans", FontKind::Sans, "Ubuntu Light"), ("Serif", FontKind::Serif, "Liberation Serif"), ("MONO", FontKind::Mono, "Hack")] {
        for engine in [Engine::Pdf, Engine::Tex] {
            let mut req = request(&env.root, engine);
            req.note = text(word);
            req.vault = text("Nothing Like It");
            let r = env.run(&req);
            assert_eq!((r.text.entry, &r.text.chosen), (Entry::Note, &Chosen::Bundled(kind)), "{word} in {engine:?}");
            assert_eq!(r.text.describe(), shown);
            assert!(r.warnings.is_empty(), "a word is a choice that ends the search");
        }
    }
    // in the other two keys the word is a family name, which is not there
    let mut req = request(&env.root, Engine::Pdf);
    req.note = Values { sans: Some("serif".into()), ..Values::default() };
    let r = env.run(&req);
    assert_eq!(r.sans.chosen, Chosen::Default("the text font"));
    assert_eq!(r.warnings, ["`print-font-sans`: the font “serif” was not found; using the text font."]);
}

// spec: PDF-034, PDF-037
#[test]
fn pdf_034_a_bad_value_is_ignored_with_a_warning_and_the_next_entry_is_used() {
    let env = Env::new("bad");
    env.vault(Spec::new("Bravo", 400, false), "fonts/Bravo.ttf");
    std::fs::write(env.root.join("fonts/readme.txt"), "x").unwrap();
    let long = "x".repeat(201);
    for (value, why) in [
        (long.as_str(), "longer than 200 characters"),
        ("Al\u{7}pha", "control character"),
        ("../Alpha.ttf", "outside the vault"),
        ("/etc/Alpha.ttf", "outside the vault"),
        ("fonts/readme.txt", "not a font file"),
        ("fonts/Missing.ttf", "was not found"),
        ("Unknown Family", "was not found"),
    ] {
        let mut req = request(&env.root, Engine::Pdf);
        req.note = text(value);
        req.vault = text("Bravo");
        let r = env.run(&req);
        assert_eq!((r.text.entry, family(&r, Role::Text).name.as_str()), (Entry::Vault, "Bravo"), "{value:?}");
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(r.warnings[0].starts_with("`print-font`: ") && r.warnings[0].contains(why) && r.warnings[0].ends_with("; using Bravo (vault, Bravo.ttf, bold and italic imitated)."), "{value:?}: {}", r.warnings[0]);
    }
    // a bad value in the vault's settings falls to the default, and says which setting it was
    let mut req = request(&env.root, Engine::Pdf);
    req.vault = Values { mono: Some("Nope".into()), ..Values::default() };
    let r = env.run(&req);
    assert_eq!(r.warnings, ["the vault's `print_font_mono`: the font “Nope” was not found; using Hack."]);
    // and so does a set's
    let mut req = request(&env.root, Engine::Tex);
    req.set = Some(("uni".into(), text("fonts/Gone.otf")));
    assert_eq!(env.run(&req).warnings, ["the template set's `font`: the font file “fonts/Gone.otf” was not found; using the template's font."]);
}

// spec: PDF-035
#[test]
fn pdf_035_a_vault_font_wins_over_a_system_font_and_the_faces_of_the_two_are_not_mixed() {
    let env = Env::new("sources");
    env.vault(Spec::new("Garamond", 400, false), "fonts/Garamond-Regular.ttf");
    env.system(Spec::new("Garamond", 400, false), "Garamond-Regular.ttf");
    env.system(Spec::new("Garamond", 700, false), "Garamond-Bold.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Garamond");
    let r = env.run(&req);
    let f = family(&r, Role::Text);
    assert_eq!(f.place, Place::Vault);
    assert!(f.face(FaceKind::Regular).unwrap().path.starts_with(&env.root));
    assert!(f.face(FaceKind::Bold).is_none(), "the system's bold is not mixed in");
}

// spec: PDF-035
#[test]
fn pdf_035_a_sets_own_fonts_count_for_the_latex_export_only_after_the_vault_and_before_the_system() {
    let env = Env::new("set-fonts");
    env.vault(Spec::new("Institute", 400, false), "templates/tex/uni/fonts/Institute.ttf");
    env.vault(Spec::new("Other", 400, false), "templates/tex/other/fonts/Other.ttf");
    env.system(Spec::new("Institute", 400, false), "Institute.ttf");
    env.system(Spec::new("Other", 400, false), "Other.ttf");
    let place = |engine: Engine, name: &str| {
        let mut req = request(&env.root, engine);
        req.note = text(name);
        req.set = Some(("uni".into(), Values::default()));
        let r = env.run(&req);
        family(&r, Role::Text).place.clone()
    };
    assert_eq!(place(Engine::Tex, "Institute"), Place::Set("uni".into()));
    assert_eq!(place(Engine::Pdf, "Institute"), Place::System, "the built-in PDF has no set");
    assert_eq!(place(Engine::Tex, "Other"), Place::System, "another set's fonts are not this set's");
    env.vault(Spec::new("Institute", 400, false), "fonts/Institute.ttf");
    assert_eq!(place(Engine::Tex, "Institute"), Place::Vault, "the vault's fonts/ come first");
}

// spec: PDF-035
#[test]
fn pdf_035_a_name_matches_ignoring_case_spaces_hyphens_and_underscores_and_never_by_prefix() {
    let env = Env::new("names");
    env.system(Spec::new("Libertinus Serif", 400, false), "L.ttf");
    for value in ["Libertinus Serif", "libertinus serif", "Libertinus-Serif", "LIBERTINUS_SERIF", "LibertinusSerif", "Libertinus  Serif"] {
        let mut req = request(&env.root, Engine::Pdf);
        req.note = text(value);
        let r = env.run(&req);
        assert!(r.warnings.is_empty(), "{value}: {:?}", r.warnings);
        assert_eq!(family(&r, Role::Text).name, "Libertinus Serif", "{value}");
    }
    for value in ["Libertinus", "Libertinus Sans", "Libertinus Seriff", "Liber"] {
        let mut req = request(&env.root, Engine::Pdf);
        req.note = text(value);
        let r = env.run(&req);
        assert_eq!(r.text.chosen, Chosen::Default("Ubuntu Light"), "{value} is a missing font, not another font");
        assert_eq!(r.warnings.len(), 1);
    }
}

// spec: PDF-035
#[test]
fn pdf_035_a_family_gives_its_four_faces_and_the_missing_ones_are_none() {
    let env = Env::new("faces");
    with_garamond(&env, false);
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("garamond");
    let r = env.run(&req);
    let f = family(&r, Role::Text);
    assert_eq!(file_names(f), garamond(|n| PathBuf::from(n)).map(|p| Some(p.to_string_lossy().into_owned())));
    assert!(!f.whole);
    assert_eq!(r.text.describe(), "Garamond (system)");
    // no bold italic and no italic: those are imitated, and the description says so
    std::fs::remove_file(env.sys.join("Garamond-BoldItalic.ttf")).unwrap();
    let r = env.run(&req);
    assert_eq!(family(&r, Role::Text).face(FaceKind::BoldItalic), None);
    assert_eq!(r.text.describe(), "Garamond (system, bold italic imitated)");
    std::fs::remove_file(env.sys.join("Garamond-Italic.ttf")).unwrap();
    std::fs::remove_file(env.sys.join("Garamond-Bold.ttf")).unwrap();
    let r = env.run(&req);
    let f = family(&r, Role::Text);
    assert!(f.face(FaceKind::Regular).is_some() && FaceKind::ALL.iter().skip(1).all(|k| f.face(*k).is_none()));
    assert_eq!(r.text.describe(), "Garamond (system, bold and italic imitated)");
}

// spec: PDF-035
#[test]
fn pdf_035_the_regular_face_is_the_upright_one_nearest_400_and_bold_is_the_nearest_to_700_from_600() {
    let env = Env::new("weights");
    for (file, weight, italic) in [
        ("Light.ttf", 300, false),
        ("Book.ttf", 400, false),
        ("Medium.ttf", 500, false),
        ("Semi.ttf", 600, false),
        ("Heavy.ttf", 800, false),
        ("Black.ttf", 900, false),
        ("It.ttf", 400, true),
        ("SemiIt.ttf", 600, true),
        ("BlackIt.ttf", 900, true),
    ] {
        env.system(Spec::new("Mixed", weight, italic), file);
    }
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Mixed");
    let r = env.run(&req);
    // 600 and 800 are as near to 700: the lighter one
    assert_eq!(file_names(family(&r, Role::Text)), ["Book.ttf", "Semi.ttf", "It.ttf", "SemiIt.ttf"].map(|s| Some(s.to_string())));
    std::fs::remove_file(env.sys.join("Semi.ttf")).unwrap();
    std::fs::remove_file(env.sys.join("SemiIt.ttf")).unwrap();
    let r = env.run(&req);
    assert_eq!(file_names(family(&r, Role::Text)), ["Book.ttf", "Heavy.ttf", "It.ttf", "BlackIt.ttf"].map(|s| Some(s.to_string())));
}

// spec: PDF-035
#[test]
fn pdf_035_a_path_names_one_file_and_gives_only_that_face() {
    let env = Env::new("path");
    with_garamond(&env, true);
    let mut req = request(&env.root, Engine::Pdf);
    for value in ["fonts/Garamond-Regular.ttf", "[[Garamond-Regular.ttf]]", "[[garamond-regular.ttf|Garamond]]", "Garamond-Regular.ttf", "./fonts/Garamond-Regular.ttf"] {
        req.note = text(value);
        let r = env.run(&req);
        assert!(r.warnings.is_empty(), "{value}: {:?}", r.warnings);
        let f = family(&r, Role::Text);
        assert_eq!((f.name.as_str(), f.place.clone()), ("Garamond", Place::Vault), "{value}");
        assert_eq!(file_names(f), [Some("Garamond-Regular.ttf".to_string()), None, None, None], "{value}: the bold and italic of the same family are not looked up");
    }
    // a path to the bold file is that file as the regular face of the text
    req.note = text("fonts/Garamond-Bold.ttf");
    assert_eq!(file_names(family(&env.run(&req), Role::Text))[0].as_deref(), Some("Garamond-Bold.ttf"));
    // a file that is not a font
    std::fs::write(env.root.join("fonts/Fake.otf"), "not a font").unwrap();
    req.note = text("fonts/Fake.otf");
    req.vault = text("Garamond");
    let r = env.run(&req);
    assert!(r.warnings[0].contains("cannot be read as a font") && r.warnings[0].contains("using Garamond"), "{:?}", r.warnings);
    assert_eq!((r.text.entry, family(&r, Role::Text).faces.iter().flatten().count()), (Entry::Vault, 4));
}

// spec: PDF-035
#[cfg(unix)]
#[test]
fn pdf_035_a_path_that_leaves_the_vault_through_a_link_is_refused() {
    let env = Env::new("link");
    let outside = folder("link-outside");
    Spec::new("Outsider", 400, false).write(&outside, "Outsider.ttf");
    std::fs::create_dir_all(env.root.join("fonts")).unwrap();
    std::os::unix::fs::symlink(&outside, env.root.join("fonts/linked")).unwrap();
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("fonts/linked/Outsider.ttf");
    let r = env.run(&req);
    assert_eq!(r.text.chosen, Chosen::Default("Ubuntu Light"));
    assert!(r.warnings[0].contains("outside the vault"), "{:?}", r.warnings);
    let _ = std::fs::remove_dir_all(&outside);
}

// spec: PDF-035
#[test]
fn pdf_035_a_name_found_nowhere_goes_to_tex_live_in_the_latex_export_and_is_missing_in_the_pdf() {
    let env = Env::new("texlive");
    let mut req = request(&env.root, Engine::Tex);
    req.note = text("TeX Gyre Pagella");
    req.vault = text("Alpha");
    let r = env.run(&req);
    assert_eq!((r.text.entry, &r.text.chosen), (Entry::Note, &Chosen::TexLive("TeX Gyre Pagella".into())));
    assert_eq!(r.text.describe(), "TeX Gyre Pagella (TeX Live, checked when exporting)");
    assert!(r.warnings.is_empty());
    req.engine = Engine::Pdf;
    let r = env.run(&req);
    assert_eq!(r.text.chosen, Chosen::Default("Ubuntu Light"), "the vault's Alpha is not there either");
    assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
}

// spec: PDF-037
#[test]
fn pdf_037_while_the_system_scan_is_not_in_a_name_is_not_called_missing() {
    let env = Env::new("pending");
    env.vault(Spec::new("Alpha", 400, false), "fonts/Alpha.ttf");
    env.system(Spec::new("Charlie", 400, false), "Charlie.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Charlie");
    req.vault = text("Alpha");
    let early = resolve(&req, &env.catalogue(false));
    assert!(early.pending && early.warnings.is_empty(), "{early:?}");
    assert_eq!(family(&early, Role::Text).name, "Alpha", "provisionally the next entry");
    let later = env.run(&req);
    assert!(!later.pending);
    assert_eq!(family(&later, Role::Text).name, "Charlie");
    // a vault font does not wait for the system
    req.note = text("Alpha");
    assert!(!resolve(&req, &env.catalogue(false)).pending);
}

// spec: PDF-037
#[test]
fn pdf_037_a_book_has_one_font_and_the_fonts_of_its_chapters_are_ignored_with_a_warning() {
    let env = Env::new("book");
    env.vault(Spec::new("Alpha", 400, false), "fonts/Alpha.ttf");
    env.vault(Spec::new("Bravo", 400, false), "fonts/Bravo.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.book = true;
    req.note = text("Alpha");
    req.chapters = vec![("One".into(), text("Bravo")), ("Two".into(), Values::default())];
    let r = env.run(&req);
    assert_eq!((r.text.entry, family(&r, Role::Text).name.as_str()), (Entry::BookTop, "Alpha"));
    assert_eq!(r.warnings, ["The chapter “One” sets its own font; it is ignored, a book has one font (the top folder note's)."]);
}

fn licensed(env: &Env, fs_type: u16, version: u16) -> Resolved {
    env.vault(Spec::new("Guarded", 400, false).licence(fs_type, version), "fonts/Guarded.ttf");
    env.vault(Spec::new("Bravo", 400, false), "fonts/Bravo.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Guarded");
    req.vault = text("Bravo");
    env.run(&req)
}

// spec: PDF-038
#[test]
fn pdf_038_a_font_that_may_not_be_embedded_is_a_warning_and_the_next_entry_is_used() {
    let env = Env::new("licence");
    for (fs_type, version, why) in [(0x0002, 4, "does not allow embedding it"), (0x0200, 4, "only bitmaps")] {
        let r = licensed(&env, fs_type, version);
        assert_eq!((r.text.entry, family(&r, Role::Text).name.as_str()), (Entry::Vault, "Bravo"), "{fs_type:#x}");
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(r.warnings[0].starts_with("`print-font`: the font “Guarded” cannot be used: ") && r.warnings[0].contains(why) && r.warnings[0].contains("using Bravo"), "{}", r.warnings[0]);
    }
    // the same by path, and by a system font
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("fonts/Guarded.ttf");
    assert!(env.run(&req).warnings[0].contains("cannot be used"));
    env.system(Spec::new("Sysguard", 400, false).licence(0x0002, 3), "Sysguard.ttf");
    req.note = text("Sysguard");
    assert!(env.run(&req).warnings[0].contains("“Sysguard” cannot be used"));
}

// spec: PDF-038
#[test]
fn pdf_038_installable_preview_print_editable_and_no_table_are_used_and_no_subsetting_means_whole() {
    let env = Env::new("licence-ok");
    for (fs_type, version, whole) in [(0x0000, 4, false), (0x0004, 4, false), (0x0008, 4, false), (0x0006, 4, false), (0x0100, 4, true), (0x0104, 3, true), (0x0100, 1, false), (0x0200, 1, false)] {
        let r = licensed(&env, fs_type, version);
        assert!(r.warnings.is_empty(), "{fs_type:#x} v{version}: {:?}", r.warnings);
        let f = family(&r, Role::Text);
        assert_eq!((f.name.as_str(), f.whole), ("Guarded", whole), "{fs_type:#x} v{version}");
        assert_eq!(r.text.describe().contains("embedded whole"), whole);
    }
    env.vault(Spec::new("Guarded", 400, false).licence(0x0002, 4).without_os2(), "fonts/Guarded.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Guarded");
    assert!(env.run(&req).warnings.is_empty(), "a font without an OS/2 table is installable");
}

// spec: PDF-038
#[test]
fn pdf_038_a_refused_face_of_a_family_refuses_the_family() {
    let env = Env::new("licence-face");
    env.system(Spec::new("Mixed", 400, false), "Mixed-Regular.ttf");
    env.system(Spec::new("Mixed", 700, false).licence(0x0002, 4), "Mixed-Bold.ttf");
    let mut req = request(&env.root, Engine::Pdf);
    req.note = text("Mixed");
    let r = env.run(&req);
    assert_eq!(r.text.chosen, Chosen::Default("Ubuntu Light"));
    assert!(r.warnings[0].contains("cannot be used"), "{:?}", r.warnings);
}

// spec: CFG-080
#[test]
fn cfg_080_the_vault_defaults_are_empty_left_out_of_the_file_and_read_from_an_old_one() {
    use crate::vault_state::VaultSettings;
    let d = VaultSettings::default();
    assert_eq!((d.print_font.as_str(), d.print_font_sans.as_str(), d.print_font_mono.as_str()), ("", "", ""));
    let json = serde_json::to_value(&d).unwrap();
    for key in ["print_font", "print_font_sans", "print_font_mono"] {
        assert!(json.get(key).is_none(), "{key} is left out of the file while empty");
    }
    // a file from before the fields reads with them empty
    let old: VaultSettings = serde_json::from_str(r#"{"version":1,"people_dir":"People"}"#).unwrap();
    assert_eq!(old.print_font, "");
    // set, they are written and read back
    let set = VaultSettings { print_font: "Garamond".into(), print_font_mono: "fonts/Mono.ttf".into(), ..VaultSettings::default() };
    let text = serde_json::to_string(&set).unwrap();
    assert!(text.contains("\"print_font\":\"Garamond\"") && text.contains("\"print_font_mono\"") && !text.contains("print_font_sans"), "{text}");
    assert_eq!(serde_json::from_str::<VaultSettings>(&text).unwrap(), set);
}
