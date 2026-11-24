//! What the guided setup does without a window (TEX-017): the accent colours read from a set, what
//! a dropped file is, the details written to `settings.toml` and kept by a re-import, the author
//! and department of the settings in the writer, and the test page.

use super::import::{import, Request, What};
use super::install::Handle;
use super::sets::*;
use super::sets_tests::{make_set, vault, META};
use super::sets_ui::VaultRef;
use super::setup::*;
use super::writer::settings_default;
use super::writer_tests::{temp_vault, tuda_set};
use crate::gitsync::TreeLock;
use std::path::Path;
use std::time::Duration;

const DEF: &str = "% colours\n\\definecolor{TUDa-0d}{cmyk/RGB/HTML}{0,0,0,.8/83,83,83/535353}\n\\definecolor{TUDa-9c}{cmyk/RGB/HTML}{.3,1,.9,0/185,15,34/B90F22}\n\\definecolor{TUDa-1a}{cmyk/RGB/HTML}{.7,.4,0,0/93,133,195/5D85C3}\n\\definecolor{TUDa-11a}{cmyk/RGB/HTML}{.6,.8,0,0/128,69,151/804597}\n\\definecolor{TUDa-8a}{cmyk/RGB/HTML}{0,.6,.8,0 /238,122,52/EE7A34}\n\\definecolor{TUDa-bad}{cmyk/RGB/HTML}{0/1,2,3/12345}\n\\definecolor{Other-1a}{HTML}{000000}\n\\colorlet{TUDa-2a}{TUDa-1a}\n";

// spec: TEX-017
#[test]
fn the_accent_colours_are_read_from_the_definitions_of_the_set_in_rows_and_columns() {
    let found = parse_accents(DEF);
    let codes: Vec<&str> = found.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes, ["1a", "8a", "11a", "9c", "0d"], "row a (by column), then c, then d; lines that are no colour of the set are left out");
    assert_eq!(found.iter().find(|s| s.code == "9c").unwrap().rgb, [185, 15, 34]);
    assert_eq!(found.iter().find(|s| s.code == "8a").unwrap().rgb, [238, 122, 52], "a space before the slash does not matter");
    let v = vault("accents");
    let dir = make_set(&v, "tuda", META, &[]);
    assert!(read_accents(&dir).is_empty(), "a set without tudacolors.def has no swatches: the colour is typed");
    std::fs::write(dir.join("tex/tudacolors.def"), DEF).unwrap();
    assert_eq!(read_accents(&dir).len(), 5);
}

// spec: TEX-017, TEX-028
#[test]
fn a_dropped_file_is_a_logo_an_archive_or_nothing_by_its_name() {
    let v = vault("classify");
    let folder = v.join("templates-folder");
    std::fs::create_dir_all(&folder).unwrap();
    for (name, want) in [("tuda_logo.pdf", Dropped::Logo), ("Logo.PNG", Dropped::Logo), ("t.zip", Dropped::Archive), ("t.tar.gz", Dropped::Archive), ("T.TGZ", Dropped::Archive), ("notes.md", Dropped::Other), ("photo.jpg", Dropped::Other)] {
        assert_eq!(classify(Path::new(name)), want, "{name}");
    }
    assert_eq!(classify(&folder), Dropped::Archive, "a folder");
}

fn vault_ref(root: &Path) -> VaultRef {
    VaultRef { root: root.to_path_buf(), tree: TreeLock::default() }
}

// spec: TEX-017, TEX-035
#[test]
fn the_details_go_to_settings_toml_and_keep_the_logos_and_a_bad_accent_changes_nothing() {
    let v = vault("details");
    let dir = make_set(&v, "tuda", META, &[("article", "name = \"A\"\nmapping = \"article\"\nclass = \"my\"\n")]);
    std::fs::write(dir.join("settings.toml"), "accent = \"9c\"\nlogo = \"mylogo.pdf\"\ndepartment_logo = \"department_logo.png\"\n").unwrap();
    let set = read_set("tuda", &dir).unwrap();
    let d = Details { accent: " 3b ".into(), language: Language::De, department: " Fachbereich Informatik ".into(), author: "Ada Lovelace".into() };
    save_details(&vault_ref(&v), &set, &d).unwrap();
    let back = load_settings(&dir).unwrap();
    assert_eq!(
        back,
        SetSettings {
            accent: "3b".into(),
            language: Language::De,
            department: "Fachbereich Informatik".into(),
            author: "Ada Lovelace".into(),
            logo: Some("mylogo.pdf".into()),
            department_logo: Some("department_logo.png".into()),
            ..SetSettings::default()
        }
    );
    assert_eq!(Details::from_settings(&back), Details { accent: "3b".into(), language: Language::De, department: "Fachbereich Informatik".into(), author: "Ada Lovelace".into() });
    // an accent that could end a command is refused and the file is as it was
    let before = std::fs::read_to_string(dir.join("settings.toml")).unwrap();
    let set = read_set("tuda", &dir).unwrap();
    let err = save_details(&vault_ref(&v), &set, &Details { accent: "9c}".into(), ..d.clone() }).unwrap_err();
    assert!(err.contains("may have only letters, digits"), "{err}");
    assert_eq!(std::fs::read_to_string(dir.join("settings.toml")).unwrap(), before);
}

// spec: TEX-017, TEX-006
#[test]
fn the_details_are_kept_when_the_set_is_imported_again() {
    let v = vault("reimport");
    let src = vault("reimport-src");
    for (name, text) in [("tex/tudapub.cls", "\\ProvidesExplClass{tudapub}{2026-06-08}{4.06}{Publications}"), ("tex/tudacolors.def", DEF)] {
        let p = src.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let request = |engine| Request { vault: v.clone(), what: What::Local(src.clone()), engine, tree: TreeLock::default(), tree_wait: Duration::from_secs(5) };
    let first = import(&request(None), &Handle::default(), &|| {}).unwrap();
    assert_eq!((first.id.as_str(), first.kinds, first.replaced), ("tuda", 11, false));
    let set = read_set("tuda", &sets_root(&v).join("tuda")).unwrap();
    assert_eq!(set.settings.accent, "9c", "the preset starts with the accent the templates are tested with");
    let d = Details { accent: "1a".into(), language: Language::De, department: "Informatik".into(), author: "Ada".into() };
    save_details(&vault_ref(&v), &set, &d).unwrap();
    let again = import(&request(None), &Handle::default(), &|| {}).unwrap();
    assert!(again.replaced);
    let back = load_settings(&sets_root(&v).join("tuda")).unwrap();
    assert_eq!(Details::from_settings(&back), d, "accent, language, department and author survive the update");
}

// spec: TEX-018, TEX-017
#[test]
fn the_author_and_the_department_of_the_settings_are_what_is_left_when_nothing_else_gives_one() {
    let s = SetSettings { author: " Ada ".into(), department: "Informatik".into(), ..SetSettings::default() };
    assert_eq!(settings_default(&s, "author").as_deref(), Some("Ada"));
    assert_eq!(settings_default(&s, "department").as_deref(), Some("Informatik"));
    assert_eq!(settings_default(&s, "title"), None, "only these two");
    assert_eq!(settings_default(&SetSettings::default(), "author"), None, "empty is none");
}

// spec: TEX-017, TEX-018
#[test]
fn the_test_page_is_a_publication_with_the_author_and_a_thesis_when_a_department_is_given() {
    let (vault, _dir) = temp_vault("setup-page", &[]);
    // no details: the publication, an author that says it is a sample
    let set = tuda_set(SetSettings { accent: "9c".into(), ..SetSettings::default() });
    let (kind, doc) = test_page(&vault, &set).unwrap();
    assert_eq!(kind, "pub");
    assert!(doc.tex.contains("\\documentclass[english,accentcolor=9c]{tudapub}") && doc.tex.contains("\\title{Shale test page}") && doc.tex.contains("\\author{Your Name}"), "{}", doc.tex);
    assert!(doc.tex.contains("A first section") || doc.tex.contains("A section"), "the sample note is written: {}", doc.tex);
    // the author of the settings, German
    let set = tuda_set(SetSettings { accent: "3b".into(), language: Language::De, author: "Ada Lovelace".into(), ..SetSettings::default() });
    let (_, doc) = test_page(&vault, &set).unwrap();
    assert!(doc.tex.contains("accentcolor=3b") && doc.tex.contains("ngerman") && doc.tex.contains("\\author{Ada Lovelace}"), "{}", doc.tex);
    // a department is shown by a kind that has it on the title page
    let set = tuda_set(SetSettings { department: "Informatik".into(), ..SetSettings::default() });
    let (kind, doc) = test_page(&vault, &set).unwrap();
    assert_eq!(kind, "thesis");
    assert!(doc.tex.contains("\\department{Informatik}") && doc.tex.contains("\\author{Your Name}"), "{}", doc.tex);
    // a set with no article kind has no test page
    let mut slides = tuda_set(SetSettings::default());
    slides.kinds.retain(|(id, _)| id == "beamer");
    assert!(test_page(&vault, &slides).unwrap_err().contains("has no article kind"));
}

// spec: TEX-059, TEX-017
#[test]
fn tex_059_the_details_of_the_guided_setup_keep_the_fonts_of_the_set() {
    let base = SetSettings { accent: "9c".into(), font: "Libertinus Serif".into(), font_sans: "Heros".into(), font_mono: "Cursor".into(), ..SetSettings::default() };
    let d = Details { accent: "3b".into(), language: Language::De, department: "Informatik".into(), author: "Ada".into() };
    let applied = d.apply(&base);
    assert_eq!((applied.accent.as_str(), applied.font.as_str(), applied.font_sans.as_str(), applied.font_mono.as_str()), ("3b", "Libertinus Serif", "Heros", "Cursor"));
}
