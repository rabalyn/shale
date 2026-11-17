//! The template sets of the vault (TEX-005, TEX-007, TEX-026): the layout, the manifests of the
//! kinds (the shipped ones of tuda-ci included), the settings of a set, the logo slot and the
//! compile against a set.

#[cfg(unix)]
use super::compile::Options;
use super::import::TUDA;
use super::sets::*;
use std::path::{Path, PathBuf};

/// A temporary folder that is a vault for these tests.
pub(super) fn vault(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-sets-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(super) const META: &str = "name = \"Test set\"\nversion = \"1.0\"\nimported_at = \"2026-11-17\"\nlogo_file = \"mylogo\"\n\n[source]\nkind = \"local\"\npath = \"/somewhere/set.zip\"\n";

/// A set in the vault by hand: `set.toml`, one class file, and these kinds.
pub(super) fn make_set(vault: &Path, id: &str, meta: &str, kinds: &[(&str, &str)]) -> PathBuf {
    let dir = sets_root(vault).join(id);
    std::fs::create_dir_all(dir.join("tex")).unwrap();
    std::fs::create_dir_all(dir.join("kinds")).unwrap();
    std::fs::write(dir.join("set.toml"), meta).unwrap();
    std::fs::write(dir.join("tex").join("my.cls"), "\\ProvidesClass{my}").unwrap();
    for (k, text) in kinds {
        std::fs::write(dir.join("kinds").join(format!("{k}.toml")), text).unwrap();
    }
    dir
}

const MINIMAL_KIND: &str = "name = \"Article\"\nmapping = \"article\"\nclass = \"my\"\n";

// spec: TEX-005
#[test]
fn a_set_is_read_from_its_layout_in_the_vault() {
    let v = vault("layout");
    let dir = make_set(&v, "mine", META, &[("article", MINIMAL_KIND), ("broken", "name = 3")]);
    std::fs::write(dir.join("settings.toml"), "accent = \"3b\"\nlanguage = \"de\"\ndepartment = \"ce\"\n").unwrap();
    // a half-made import is not a set, a folder without set.toml is one that cannot be read
    std::fs::create_dir_all(sets_root(&v).join(".mine.staging")).unwrap();
    std::fs::create_dir_all(sets_root(&v).join("empty")).unwrap();
    let found = scan(&v);
    assert_eq!(found.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["empty", "mine"]);
    assert!(found[0].info.as_ref().unwrap_err().contains("set.toml cannot be read"));
    let s = found[1].info.as_ref().unwrap();
    assert_eq!((s.meta.name.as_str(), s.meta.version.as_deref(), s.meta.imported_at.as_str()), ("Test set", Some("1.0"), "2026-11-17"));
    assert_eq!(s.meta.source, Source::Local { path: "/somewhere/set.zip".into() });
    assert_eq!((s.settings.accent.as_str(), s.settings.language, s.settings.department.as_str()), ("3b", Language::De, "ce"));
    assert_eq!(s.kinds.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["article"]);
    assert!(s.problems.len() == 1 && s.problems[0].starts_with("kinds/broken.toml:"), "{:?}", s.problems);
    assert_eq!(s.dir, dir);
    assert_eq!(sets_root(&v), v.join("templates").join("tex"));
    std::fs::remove_dir_all(&v).unwrap();
}

// spec: TEX-005
#[test]
fn a_manifest_has_defaults_and_refuses_what_could_break_the_document() {
    let k = KindManifest::parse(MINIMAL_KIND).unwrap();
    assert_eq!((k.name.as_str(), k.mapping, k.class.as_str()), ("Article", Mapping::Article, "my"));
    assert!(k.options.is_empty() && k.preamble.is_empty() && k.fields.is_empty() && !k.logo && k.description.is_empty());
    let full = KindManifest::parse("name = \"X\"\nmapping = \"letter\"\nclass = \"c\"\nlogo = true\noptions = [\"a\"]\npreamble = [\"\\\\usepackage{x}\"]\n[[fields]]\nkey = \"title\"\ncommand = \"title\"\nrequired = true\ndefault = \"\\\\today\"\nlabel = \"T\"\n[[fields]]\nkey = \"subject\"\nkomavar = \"subject\"\n").unwrap();
    assert!(full.logo && full.fields[0].required && full.fields[0].default == "\\today" && !full.fields[1].required);
    assert_eq!((full.fields[0].command.as_deref(), full.fields[1].komavar.as_deref(), full.fields[1].command.as_deref()), (Some("title"), Some("subject"), None));
    for (bad, why) in [
        ("name = \"X\"\nmapping = \"poem\"\nclass = \"c\"\n", "unknown variant"),
        ("name = \"X\"\nmapping = \"article\"\n", "class"),
        ("name = \"X\"\nmapping = \"article\"\nclass = \"c}\\\\input{x\"\n", "not a class name"),
        ("name = \"X\"\nmapping = \"article\"\nclass = \"c\"\n[[fields]]\nkey = \"\"\n", "no key"),
        ("name = \"X\"\nmapping = \"article\"\nclass = \"c\"\n[[fields]]\nkey = \"a\"\ncommand = \"x{y\"\n", "not the name of a command"),
        ("name = \"X\"\nmapping = \"article\"\nclass = \"c\"\n[[fields]]\nkey = \"a\"\nkomavar = \"\"\n", "not the name of a command"),
    ] {
        let e = KindManifest::parse(bad).unwrap_err();
        assert!(e.contains(why), "{bad:?}: {e}");
    }
}

// spec: TEX-005
#[test]
fn the_settings_have_defaults_are_checked_and_survive_a_save() {
    let v = vault("settings");
    let d = v.join("set");
    std::fs::create_dir_all(&d).unwrap();
    assert_eq!(load_settings(&d).unwrap(), SetSettings::default(), "no file: the defaults");
    assert_eq!((SetSettings::default().language, SetSettings::default().accent.as_str(), SetSettings::default().logo), (Language::En, "", None));
    let s = SetSettings { accent: "9c".into(), language: Language::De, department: "Fachbereich Informatik".into(), logo: Some("tuda_logo.pdf".into()) };
    save_settings(&d, &s).unwrap();
    assert_eq!(load_settings(&d).unwrap(), s);
    let text = std::fs::read_to_string(d.join("settings.toml")).unwrap();
    assert!(text.starts_with('#') && text.contains("accent = \"9c\"") && text.contains("language = \"de\""), "{text}");
    std::fs::write(d.join("settings.toml"), "accent = \"9c\"\n").unwrap();
    assert_eq!(load_settings(&d).unwrap().language, Language::En, "a missing key is its default");
    for (bad, why) in [
        ("accent = \"9c,x=y\"\n", "accent colour"),
        ("accent = \"{x}\"\n", "accent colour"),
        ("logo = \"../x.pdf\"\n", "plain file name"),
        ("logo = \".hidden\"\n", "plain file name"),
        ("language = \"fr\"\n", "settings.toml"),
        ("not toml [", "settings.toml"),
    ] {
        std::fs::write(d.join("settings.toml"), bad).unwrap();
        assert!(load_settings(&d).unwrap_err().contains(why), "{bad:?}");
    }
    std::fs::remove_dir_all(&v).unwrap();
}

/// The classes tuda-ci makes (`tests/tex-env` checks the imported files for each).
const TUDA_CLASSES: [&str; 7] = ["tudapub", "tudaexercise", "tudabeamer", "tudaletter", "tudaposter", "tudasciposter", "tudaleaflet"];

// spec: TEX-005
#[test]
fn every_shipped_tuda_kind_parses_and_names_a_class_of_tuda_ci() {
    let ids: Vec<&str> = TUDA.kinds.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["pub", "report", "exercise", "thesis", "phd", "beamer", "letter", "poster", "sciposter", "leaflet", "announcement"], "the 11 kinds");
    for (id, text) in TUDA.kinds {
        let k = KindManifest::parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(TUDA_CLASSES.contains(&k.class.as_str()), "{id}: class {}", k.class);
        assert!(k.logo && !k.name.is_empty() && !k.description.is_empty(), "{id}");
        assert!(k.options.iter().any(|o| o == "{language}") && k.options.iter().any(|o| o == "accentcolor={accent}"), "{id}: language and accent come from the settings");
        assert!(k.fields.iter().any(|f| f.key == "title" || k.mapping == Mapping::Letter && f.key == "subject"), "{id}: a title");
        let keys: Vec<&str> = k.fields.iter().map(|f| f.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "{id}: a key twice");
        assert!(k.fields.iter().all(|f| f.command.is_some() || f.komavar.is_some() || k.mapping == Mapping::Letter), "{id}: every field goes somewhere");
    }
    let mapping = |id: &str| KindManifest::parse(TUDA.kinds.iter().find(|(i, _)| *i == id).unwrap().1).unwrap().mapping;
    assert_eq!([mapping("pub"), mapping("report"), mapping("exercise")], [Mapping::Article; 3]);
    assert_eq!([mapping("thesis"), mapping("phd")], [Mapping::Book; 2]);
    assert_eq!((mapping("beamer"), mapping("letter")), (Mapping::Beamer, Mapping::Letter));
    assert_eq!([mapping("poster"), mapping("sciposter"), mapping("leaflet"), mapping("announcement")], [Mapping::Poster; 4]);
    // the fields of the demo files of the kinds that have a person behind them
    let fields = |id: &str| KindManifest::parse(TUDA.kinds.iter().find(|(i, _)| *i == id).unwrap().1).unwrap().fields.into_iter().map(|f| f.key).collect::<Vec<_>>();
    for k in ["reviewer", "department", "institute", "group", "submissiondate", "examdate"] {
        assert!(fields("thesis").contains(&k.to_string()) && fields("phd").contains(&k.to_string()), "{k}");
    }
    assert!(fields("letter").iter().any(|k| k == "recipient") && fields("exercise").iter().any(|k| k == "sheet"));
}

// spec: TEX-005
#[test]
fn the_settings_are_filled_into_options_and_preamble_and_an_empty_value_drops_its_option() {
    let k = KindManifest::parse(TUDA.kinds[0].1).unwrap();
    let mut s = SetSettings { accent: "3b".into(), ..SetSettings::default() };
    assert_eq!(k.class_options(&s), ["english", "accentcolor=3b"]);
    assert!(k.preamble_lines(&s).contains(&"\\usepackage[english]{babel}".to_string()));
    s.language = Language::De;
    assert_eq!(k.class_options(&s)[0], "ngerman");
    assert!(k.preamble_lines(&s)[0].contains("[ngerman]"));
    s.accent = String::new();
    assert_eq!(k.class_options(&s), ["ngerman"], "no accent colour: no accentcolor= option");
}

// spec: TEX-005
#[test]
fn text_for_tex_has_its_special_characters_escaped() {
    assert_eq!(tex_escape("R&D #1 50% {a} $x$ a_b ^ ~ \\x"), "R\\&D \\#1 50\\% \\{a\\} \\$x\\$ a\\_b \\textasciicircum{} \\textasciitilde{} \\textbackslash{}x");
    assert_eq!(tex_escape("Fachbereich Elektrotechnik für Ärzte"), "Fachbereich Elektrotechnik für Ärzte", "Unicode stays");
    assert_eq!(id_from("Tuda Latex_Templates.zip"), "tuda-latex_templates");
    assert_eq!(id_from("my-repo.git"), "my-repo");
    assert_eq!(id_from("x.tar.gz"), "x");
    assert_eq!(id_from("..."), "templates");
    assert_eq!(id_from(&"a".repeat(80)).len(), 40);
}

// spec: TEX-026
#[test]
fn a_sample_document_of_every_mapping_has_its_class_options_fields_and_body() {
    let s = SetSettings { accent: "9c".into(), department: "R&D".into(), ..SetSettings::default() };
    let doc = |id: &str| sample_document(&KindManifest::parse(TUDA.kinds.iter().find(|(i, _)| *i == id).unwrap().1).unwrap(), &s);
    let pubd = doc("pub");
    assert!(
        pubd.starts_with("\\RequirePackage{pdfmanagement}\n\\SetKeys[document/metadata]{pdfstandard=a-2b,pdfversion=1.7,lang=en}\n\\documentclass[english,accentcolor=9c]{tudapub}\n") && pubd.contains("\\usepackage[english]{babel}\n"),
        "{pubd}"
    );
    assert!(sample_document(&KindManifest::parse(TUDA.kinds[0].1).unwrap(), &SetSettings { language: Language::De, ..s.clone() }).contains("lang=de}\n"), "the language code of the PDF metadata");
    assert!(pubd.contains("\\title{Shale test page}\n") && pubd.contains("\\author{Shale}\n") && !pubd.contains("\\date"), "{pubd}");
    assert!(pubd.contains("\\begin{document}\n\\maketitle\n\\section{First section}") && pubd.trim_end().ends_with("\\end{document}"));
    let thesis = doc("thesis");
    assert!(thesis.contains("\\department{R\\&D}") && thesis.contains("\\submissiondate{\\today}") && thesis.contains("\\chapter{First chapter}"), "{thesis}");
    assert!(doc("beamer").contains("\\begin{frame}{First frame}"));
    let letter = doc("letter");
    assert!(
        letter.contains("\\begin{letter}{Sample Recipient\\\\Sample Street 1") && letter.contains("\\setkomavar{subject}{Shale test page}") && letter.contains("\\opening{Dear Sir or Madam,}") && letter.contains("\\closing{Kind regards}"),
        "{letter}"
    );
    assert!(doc("sciposter").contains("\\begin{tcbposter}") && doc("sciposter").contains("\\begin{posterboxenv}"));
    assert!(doc("poster").contains("\\maketitle") && !doc("poster").contains("tcbposter"));
    assert!(doc("announcement").starts_with("\\documentclass[english,accentcolor=9c,paper=a4,type=announcement,footer=true]{tudaposter}"));
    let no_accent = sample_document(&KindManifest::parse(TUDA.kinds[0].1).unwrap(), &SetSettings::default());
    assert!(no_accent.contains("\\documentclass[english]{tudapub}"));
    let no_options = sample_document(&KindManifest::parse("name=\"x\"\nmapping=\"article\"\nclass=\"c\"\n").unwrap(), &s);
    assert!(no_options.starts_with("\\documentclass{c}\n"), "{no_options}");
}

// spec: TEX-026
#[test]
fn the_test_page_uses_the_pub_kind_else_the_first_article_like_one() {
    let v = vault("testkind");
    make_set(&v, "a", META, &[("zzz", "name=\"Z\"\nmapping=\"beamer\"\nclass=\"my\"\n"), ("aaa", MINIMAL_KIND), ("pub", MINIMAL_KIND)]);
    make_set(&v, "b", META, &[("aaa", MINIMAL_KIND)]);
    make_set(&v, "c", META, &[("slides", "name=\"Z\"\nmapping=\"beamer\"\nclass=\"my\"\n")]);
    let found = scan(&v);
    let kind_of = |i: usize| test_kind(found[i].info.as_ref().unwrap()).map(|(id, _)| id.clone());
    assert_eq!((kind_of(0), kind_of(1), kind_of(2)), (Some("pub".into()), Some("aaa".into()), None));
    std::fs::remove_dir_all(&v).unwrap();
}

// spec: TEX-007
#[test]
fn the_placeholder_logo_is_a_valid_one_page_pdf() {
    let pdf = placeholder_logo_pdf();
    assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
    let doc = hayro::hayro_syntax::Pdf::new(pdf).unwrap_or_else(|e| panic!("not a readable PDF: {e:?}"));
    assert_eq!(doc.pages().len(), 1);
    // the cross-reference table points at the objects it names
    let text = String::from_utf8(placeholder_logo_pdf()).unwrap();
    for (n, line) in text.split("xref\n").nth(1).unwrap().lines().skip(2).take(4).enumerate() {
        let offset: usize = line[..10].parse().unwrap();
        assert!(text[offset..].starts_with(&format!("{} 0 obj", n + 1)), "object {}", n + 1);
    }
}

fn a_pdf() -> Vec<u8> {
    placeholder_logo_pdf()
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

// spec: TEX-007
#[test]
fn choosing_a_logo_copies_it_into_the_slot_records_it_and_trashes_the_one_it_replaces() {
    let v = vault("logo");
    let dir = make_set(&v, "mine", META, &[("article", MINIMAL_KIND)]);
    let set = || read_set("mine", &dir).unwrap();
    assert_eq!(set().logo, LogoStatus::Missing);
    assert_eq!(LogoStatus::Missing.text(), "logo missing: exports are not CI-conformant");
    let from = v.join("Downloads").join("any name.pdf");
    std::fs::create_dir_all(from.parent().unwrap()).unwrap();
    std::fs::write(&from, a_pdf()).unwrap();
    assert_eq!(choose_logo(&v, &set(), &from).unwrap(), "mylogo.pdf");
    assert_eq!(std::fs::read(dir.join("logo/mylogo.pdf")).unwrap(), a_pdf());
    assert_eq!(set().logo, LogoStatus::Present("mylogo.pdf".into()));
    assert_eq!(set().settings.logo.as_deref(), Some("mylogo.pdf"), "recorded in settings.toml");
    assert!(std::fs::read_to_string(dir.join("settings.toml")).unwrap().contains("mylogo.pdf"));
    assert_eq!(LogoStatus::Present("mylogo.pdf".into()).text(), "logo: mylogo.pdf");
    // the same file again changes nothing and trashes nothing
    choose_logo(&v, &set(), &from).unwrap();
    assert!(!v.join(".trash").exists());
    // a PNG replaces the PDF; the PDF goes to the vault's trash, so exactly one logo is in the slot
    let png = v.join("logo.png");
    std::fs::write(&png, PNG).unwrap();
    assert_eq!(choose_logo(&v, &set(), &png).unwrap(), "mylogo.png");
    assert!(!dir.join("logo/mylogo.pdf").exists() && dir.join("logo/mylogo.png").is_file());
    let day = std::fs::read_dir(v.join(".trash")).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(day.join("templates/tex/mine/logo/mylogo.pdf")).unwrap(), a_pdf());
    // a hand-made logo is found without settings.toml naming it
    std::fs::remove_file(dir.join("settings.toml")).ok();
    assert_eq!(set().logo, LogoStatus::Present("mylogo.png".into()));
    std::fs::remove_dir_all(&v).unwrap();
}

// spec: TEX-007
#[test]
fn a_logo_that_is_not_a_pdf_or_png_or_too_big_or_for_a_set_without_a_slot_is_refused() {
    let v = vault("logo-bad");
    let dir = make_set(&v, "mine", META, &[]);
    let set = read_set("mine", &dir).unwrap();
    let text = v.join("logo.txt");
    std::fs::write(&text, "not a picture").unwrap();
    assert!(choose_logo(&v, &set, &text).unwrap_err().contains("PDF or a PNG"));
    assert!(choose_logo(&v, &set, &v.join("missing.pdf")).is_err());
    assert!(choose_logo(&v, &set, &v).unwrap_err().contains("not a file"));
    let big = v.join("big.pdf");
    std::fs::write(&big, vec![b'%'; (MAX_LOGO + 1) as usize]).unwrap();
    assert!(choose_logo(&v, &set, &big).unwrap_err().contains("at most 5 MB"));
    assert!(!dir.join("logo").exists() || std::fs::read_dir(dir.join("logo")).unwrap().next().is_none(), "nothing was put in the slot");
    let no_slot = make_set(&v, "plain", &META.replace("logo_file = \"mylogo\"\n", ""), &[]);
    let plain = read_set("plain", &no_slot).unwrap();
    assert_eq!(plain.logo, LogoStatus::NotNeeded);
    assert_eq!(LogoStatus::NotNeeded.text(), "no logo needed");
    let pdf = v.join("ok.pdf");
    std::fs::write(&pdf, a_pdf()).unwrap();
    assert!(choose_logo(&v, &plain, &pdf).unwrap_err().contains("no logo slot"));
    std::fs::remove_dir_all(&v).unwrap();
}

#[cfg(unix)]
mod fake {
    use super::*;
    use crate::pdf::tex::compile_tests::fake::{script, Fake};

    /// A LuaLaTeX that records what a compile gives it: the search path, what is in the work
    /// folder (a copy of the logo it finds there), and makes a PDF.
    fn recording_lualatex(f: &Fake) {
        let st = f.state();
        script(
            &f.dir.join("bin"),
            "lualatex",
            &format!(
                "S='{}'\nenv > \"$S/env\"\nls > \"$S/ls\"\nfor l; do last=$l; done\nstem=${{last%.tex}}\ncp \"$last\" \"$S/main.tex\"\nif [ -f mylogo.pdf ]; then cp mylogo.pdf \"$S/logo-seen.pdf\"; fi\necho ok > \"$stem.log\"\necho '%PDF-1.5 fake' > \"$stem.pdf\"",
                st.display()
            ),
        );
    }

    fn texinputs(f: &Fake) -> Vec<PathBuf> {
        let env = f.get("env");
        let v = env.lines().find_map(|l| l.strip_prefix("TEXINPUTS=")).expect("TEXINPUTS");
        std::env::split_paths(v).collect()
    }

    // spec: TEX-007, TEX-026
    #[test]
    fn a_compile_against_a_set_searches_its_tex_and_logo_folders_and_a_missing_logo_is_a_placeholder_in_the_work_folder() {
        let f = Fake::new("set-compile");
        recording_lualatex(&f);
        let v = vault("set-compile");
        let dir = make_set(&v, "mine", META, &[("article", MINIMAL_KIND)]);
        let set = read_set("mine", &dir).unwrap();
        let out = compile_with_set(&f.engine(), &set, "page.tex", "\\documentclass{my}", &[("extra.bib", b"@book{}")], Options::default()).unwrap();
        assert!(out.output.pdf.starts_with(b"%PDF") && out.placeholder_logo);
        assert_eq!(texinputs(&f), [dir.join("tex//"), PathBuf::new()], "the set's tex folder, then TeX's own; no logo folder yet");
        let ls = f.get("ls");
        assert!(ls.lines().any(|l| l == "mylogo.pdf") && ls.lines().any(|l| l == "extra.bib") && ls.lines().any(|l| l == "page.tex"), "{ls}");
        assert_eq!(f.get("main.tex"), "\\documentclass{my}");
        assert_eq!(std::fs::read(f.state().join("logo-seen.pdf")).unwrap(), placeholder_logo_pdf(), "the class finds the placeholder");
        assert!(!dir.join("logo").exists(), "the placeholder is never written into the vault");
        // with a logo in the slot the logo folder is searched and no placeholder is made
        let logo = v.join("l.pdf");
        std::fs::write(&logo, a_pdf()).unwrap();
        choose_logo(&v, &set, &logo).unwrap();
        let set = read_set("mine", &dir).unwrap();
        let out = compile_with_set(&f.engine(), &set, "page.tex", "x", &[], Options::default()).unwrap();
        assert!(!out.placeholder_logo);
        assert_eq!(texinputs(&f), [dir.join("tex//"), dir.join("logo//"), PathBuf::new()]);
        assert!(!f.get("ls").lines().any(|l| l == "mylogo.pdf"), "no placeholder next to the real logo: {}", f.get("ls"));
        // a set that needs no logo gets none
        let plain = read_set("mine", &make_set(&v, "plain", &META.replace("logo_file = \"mylogo\"\n", ""), &[])).unwrap();
        assert!(!compile_with_set(&f.engine(), &plain, "page.tex", "x", &[], Options::default()).unwrap().placeholder_logo);
        assert!(!f.get("ls").lines().any(|l| l == "mylogo.pdf"));
        std::fs::remove_dir_all(&v).unwrap();
    }

    // spec: TEX-026
    #[test]
    fn a_failing_compile_against_a_set_says_why() {
        let f = Fake::new("set-fail");
        f.lualatex();
        f.put("log.last", "! LaTeX Error: File `my.cls' not found.\n");
        f.put("exit", "1");
        let v = vault("set-fail");
        let set = read_set("mine", &make_set(&v, "mine", META, &[])).unwrap();
        let e = compile_with_set(&f.engine(), &set, "page.tex", "x", &[], Options::default()).unwrap_err();
        assert!(e.problems.iter().any(|p| p.text() == "class my is missing"), "{e}");
        // names that cannot be written (a main file that exists twice) are errors, not panics
        let e = compile_with_set(&f.engine(), &set, "page.tex", "x", &[("page.tex", b"again")], Options::default()).unwrap_err();
        assert!(e.headline.contains("page.tex could not be written"), "{e}");
        std::fs::remove_dir_all(&v).unwrap();
    }
}
