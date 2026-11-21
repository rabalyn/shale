//! The template sets against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless
//! asked for: `cargo test -- --ignored tex_env`, `cargo test -- --ignored tex_import`): the pinned
//! tuda-ci archive of the test stack is imported as a local archive (docstrip runs in the
//! container, no network) and a document of every one of the 11 kinds is compiled from the set in
//! the vault, without a logo (the placeholder) and with one.

use super::compile::Options;
use super::compile_tests::container_engine;
use super::engine::Engine;
use super::import::*;
use super::install::Handle;
use super::pdf_text::pictures_drawn;
use super::sets::*;
use crate::gitsync::TreeLock;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The folder the container mounts: the temporary root of the compiles. The vault lives in it
/// too, so the container sees the set's files at the same path.
/// Options for a compile in the container: a fresh container builds the font database before it
/// starts, which takes long when the machine is busy, so a run may take much longer than in a window.
pub(super) fn patient() -> Options {
    Options { timeout: Duration::from_secs(900), ..Options::default() }
}

pub(super) fn mounted_root() -> PathBuf {
    crate::fsutil::private_temp_root("shale-tex").unwrap()
}

pub(super) fn cached_archive() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tex-env/cache/tuda-ci.tar.gz");
    assert!(path.is_file(), "{} is not there: run tests/tex-env/tex-env.sh build", path.display());
    path
}

pub(super) fn request(vault: &Path, what: What, engine: Engine) -> Request {
    Request { vault: vault.to_path_buf(), what, engine: Some(engine), tree: TreeLock::default(), tree_wait: Duration::from_secs(5) }
}

/// The vault with the imported set, made once for all tests of the run.
pub(super) fn imported_vault() -> &'static (PathBuf, Imported) {
    static VAULT: OnceLock<(PathBuf, Imported)> = OnceLock::new();
    VAULT.get_or_init(|| {
        let root = mounted_root();
        let vault = root.join(format!("vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);
        std::fs::create_dir_all(&vault).unwrap();
        let archive = cached_archive();
        let got = import(&request(&vault, What::Local(archive), container_engine(&root)), &Handle::default(), &|| {}).unwrap_or_else(|e| panic!("the import failed: {e}"));
        (vault, got)
    })
}

pub(super) fn the_set() -> SetInfo {
    let (vault, _) = imported_vault();
    scan(vault).into_iter().find(|e| e.id == "tuda").expect("the set tuda").info.unwrap_or_else(|e| panic!("{e}"))
}

/// A small blue PNG, the stand-in for a department's logo: no real logo is in the repository.
pub(super) fn a_department_logo_png() -> Vec<u8> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(120, 40, image::Rgb([0, 90, 160])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    png.into_inner()
}

/// The imported set with a department logo in its slot (TEX-035), a copy of the vault's set in a
/// vault of its own, made once for all tests of the run.
pub(super) fn the_set_with_a_department_logo() -> SetInfo {
    static VAULT: OnceLock<PathBuf> = OnceLock::new();
    let own = VAULT.get_or_init(|| {
        let (vault, _) = imported_vault();
        let own = mounted_root().join(format!("vault-dept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&own);
        std::fs::create_dir_all(own.join("templates")).unwrap();
        crate::pdf::tex::archive::copy_folder(&vault.join("templates"), &own.join("templates")).unwrap();
        let logo = own.join("department-source.png");
        std::fs::write(&logo, a_department_logo_png()).unwrap();
        let set = read_set("tuda", &own.join("templates/tex/tuda")).unwrap();
        assert_eq!(choose_department_logo(&own, &set, &logo).unwrap(), "department_logo.png");
        own
    });
    let set = read_set("tuda", &own.join("templates/tex/tuda")).unwrap();
    assert_eq!(set.department_logo.as_deref(), Some("department_logo.png"));
    set
}

/// A logo that is not the placeholder (another grey).
fn a_real_logo() -> Vec<u8> {
    String::from_utf8(placeholder_logo_pdf()).unwrap().replace("0.75 g", "0.20 g").into_bytes()
}

fn pdf_title_and_pages(pdf: Vec<u8>) -> (Option<String>, usize) {
    let doc = hayro::hayro_syntax::Pdf::new(pdf).unwrap_or_else(|e| panic!("not a readable PDF: {e:?}"));
    // a text string of a PDF: UTF-16 with a byte order mark, else (nearly) Latin-1
    let text = |b: &[u8]| match b.strip_prefix(&[0xFE, 0xFF]) {
        Some(utf16) => String::from_utf16_lossy(&utf16.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()),
        None => b.iter().map(|&c| c as char).collect(),
    };
    (doc.metadata().title.as_ref().map(|t| text(t)).filter(|t| !t.is_empty()), doc.pages().len())
}

// spec: TEX-006, TEX-021
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_the_pinned_tuda_ci_archive_is_imported_with_docstrip_and_every_class_of_the_kinds_exists() {
    crate::env_lock::hold();
    // the archive of the cache is the pinned one
    let bytes = std::fs::read(cached_archive()).unwrap();
    verify_sha256(&bytes, PRESET_SHA256).expect("the cached archive is the pinned one");
    let (vault, got) = imported_vault();
    assert_eq!((got.id.as_str(), got.name.as_str(), got.kinds, got.replaced, got.logo_needed), ("tuda", "TU Darmstadt (tuda-ci)", 11, false, true));
    assert_eq!(got.version.as_deref(), Some("4.06"), "read from the class that docstrip made");
    let set = the_set();
    assert_eq!((set.kinds.len(), set.problems.len(), set.logo.clone()), (11, 0, LogoStatus::Missing), "{:?}", set.problems);
    for (id, kind) in &set.kinds {
        assert!(set.dir.join("tex").join(format!("{}.cls", kind.class)).is_file(), "kind {id}: class {} is not in tex/", kind.class);
    }
    // the generated files are in the set, the sources and the demos are not, and nothing of the logo
    for f in ["tudapub.cls", "tudabeamer.cls", "tudaletter.cls", "tudasciposter.cls", "tudacolors.def", "tudafonts.sty", "beamerthemeTUDa.sty", "tuda-a4paper.clo"] {
        assert!(set.dir.join("tex").join(f).is_file(), "{f}");
    }
    assert!(!set.dir.join("tex/tuda-ci.dtx").exists() && !set.dir.join("examples").exists() && !set.dir.join("logo").exists() && !set.dir.join("tex/tuda_logo.pdf").exists());
    assert_eq!(set.meta.source, Source::Local { path: cached_archive().to_string_lossy().into_owned() });
    let _ = vault;
}

/// Compile every kind of the set and say what failed.
fn compile_every_kind(set: &SetInfo, expect_placeholder: bool) {
    compile_every_kind_with(set, expect_placeholder, false);
}

/// The same, with or without the department logo in the sample documents.
fn compile_every_kind_with(set: &SetInfo, expect_placeholder: bool, department_logo: bool) {
    let root = mounted_root();
    let engine = container_engine(&root);
    let mut failed = vec![];
    for (id, kind) in &set.kinds {
        let doc = sample_document(kind, &set.settings, department_logo);
        match compile_with_set(&engine, set, "page.tex", &doc, &[], patient()) {
            Err(e) => failed.push(format!("{id}: {e}\n{}\n--- the document:\n{doc}", e.log_tail)),
            Ok(out) => {
                if out.placeholder_logo != expect_placeholder {
                    failed.push(format!("{id}: placeholder_logo is {}", out.placeholder_logo));
                }
                let (title, pages) = pdf_title_and_pages(out.output.pdf);
                if pages == 0 {
                    failed.push(format!("{id}: the PDF has no page"));
                }
                // the title is in the PDF's metadata where the class sets it from \title
                if let Some(t) = title {
                    if !t.contains("Shale test page") && !matches!(id.as_str(), "letter") {
                        failed.push(format!("{id}: the PDF's title is {t:?}"));
                    }
                }
                eprintln!("kind {id}: {pages} page(s)");
            }
        }
    }
    assert!(failed.is_empty(), "{} of {} kinds failed:\n{}", failed.len(), set.kinds.len(), failed.join("\n=====\n"));
}

// spec: TEX-005, TEX-007, TEX-026, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_every_kind_of_the_imported_tuda_set_compiles_with_the_placeholder_logo() {
    crate::env_lock::hold();
    let set = the_set();
    assert_eq!(set.logo, LogoStatus::Missing, "no logo in the slot");
    compile_every_kind(&set, true);
    assert!(!set.dir.join("logo").exists(), "the placeholder is not written into the vault");
}

// spec: TEX-005, TEX-007, TEX-026, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_every_kind_of_the_imported_tuda_set_compiles_with_a_logo_in_the_slot_and_german_settings() {
    crate::env_lock::hold();
    // a copy of the vault's set with a logo, in German with another accent colour
    let (vault, _) = imported_vault();
    let root = mounted_root();
    let own = root.join(format!("vault-logo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&own);
    std::fs::create_dir_all(own.join("templates")).unwrap();
    crate::pdf::tex::archive::copy_folder(&vault.join("templates"), &own.join("templates")).unwrap();
    let logo = own.join("logo-source.pdf");
    std::fs::write(&logo, a_real_logo()).unwrap();
    let set = read_set("tuda", &own.join("templates/tex/tuda")).unwrap();
    assert_eq!(choose_logo(&own, &set, &logo).unwrap(), "tuda_logo.pdf");
    let mut set = read_set("tuda", &own.join("templates/tex/tuda")).unwrap();
    assert_eq!(set.logo, LogoStatus::Present("tuda_logo.pdf".into()));
    set.settings = SetSettings { accent: "3b".into(), language: Language::De, department: "Fachbereich Informatik & Co".into(), author: "Ada & Bob".into(), logo: set.settings.logo.clone(), department_logo: None };
    compile_every_kind(&set, false);
    let _ = std::fs::remove_dir_all(&own);
}

// spec: TEX-035, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_every_kind_with_a_department_logo_compiles_and_the_pdf_shows_the_logo_where_the_kind_has_a_place() {
    crate::env_lock::hold();
    let set = the_set_with_a_department_logo();
    // an empty slot is no warning and no placeholder (the TU logo's is still the placeholder)
    let empty = the_set();
    assert!(empty.department_logo.is_none() && empty.department_logo_text().is_none());
    let engine = container_engine(&mounted_root());
    let mut failed = vec![];
    for (id, kind) in &set.kinds {
        let pictures = |department_logo: bool| {
            let doc = sample_document(kind, &set.settings, department_logo);
            match compile_with_set(&engine, &set, "page.tex", &doc, &[], patient()) {
                Ok(out) => Ok(pictures_drawn(out.output.pdf)),
                Err(e) => Err(format!("{id} (department logo {department_logo}): {e}\n{}\n--- the document:\n{doc}", e.log_tail)),
            }
        };
        match (pictures(false), pictures(true)) {
            (Ok(without), Ok(with)) => {
                let has_place = !kind.department_logo.is_empty();
                eprintln!("kind {id}: {without} picture(s) without, {with} with the department logo");
                if has_place && with <= without || !has_place && with != without {
                    failed.push(format!("{id}: {without} pictures without the department logo and {with} with it (the kind has a place: {has_place})"));
                }
            }
            (a, b) => failed.extend([a.err(), b.err()].into_iter().flatten()),
        }
    }
    assert!(failed.is_empty(), "{} of {} kinds failed:\n{}", failed.len(), set.kinds.len(), failed.join("\n=====\n"));
    // the same, in German with another accent colour and a TeX special character in the settings
    let mut german = set.clone();
    german.settings = SetSettings { accent: "3b".into(), language: Language::De, department: "Fachbereich Informatik & Co".into(), author: "Ada & Bob".into(), ..set.settings.clone() };
    compile_every_kind_with(&german, true, true);
}

// spec: TEX-006
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_re_import_of_the_cached_archive_keeps_the_logo_and_the_settings() {
    crate::env_lock::hold();
    let root = mounted_root();
    let vault = root.join(format!("vault-again-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&vault);
    std::fs::create_dir_all(&vault).unwrap();
    let req = request(&vault, What::Local(cached_archive()), container_engine(&root));
    import(&req, &Handle::default(), &|| {}).unwrap();
    let dir = vault.join("templates/tex/tuda");
    std::fs::write(dir.join("settings.toml"), "accent = \"4c\"\nlanguage = \"de\"\n").unwrap();
    std::fs::create_dir_all(dir.join("logo")).unwrap();
    std::fs::write(dir.join("logo/tuda_logo.pdf"), a_real_logo()).unwrap();
    let again = import(&req, &Handle::default(), &|| {}).unwrap();
    assert!(again.replaced && again.kinds == 11);
    assert_eq!(std::fs::read_to_string(dir.join("settings.toml")).unwrap(), "accent = \"4c\"\nlanguage = \"de\"\n");
    assert_eq!(std::fs::read(dir.join("logo/tuda_logo.pdf")).unwrap(), a_real_logo());
    assert!(vault.join(".trash").is_dir(), "the old set is in the trash");
    let _ = std::fs::remove_dir_all(&vault);
}

// spec: TEX-006
#[test]
#[ignore = "needs the network and the container: tests/tex-env/tex-env.sh build"]
fn tex_import_the_pinned_preset_is_downloaded_from_github_checked_by_its_sha_and_laid_out() {
    crate::env_lock::hold();
    let root = mounted_root();
    let vault = root.join(format!("vault-github-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&vault);
    std::fs::create_dir_all(&vault).unwrap();
    let got = import(&request(&vault, What::Preset, container_engine(&root)), &Handle::default(), &|| {}).unwrap_or_else(|e| panic!("the preset import failed: {e}"));
    assert_eq!((got.id.as_str(), got.kinds, got.version.as_deref()), ("tuda", 11, Some("4.06")));
    let set = scan(&vault).remove(0).info.unwrap();
    assert_eq!(set.meta.source, Source::Github { repo: PRESET_REPO.into(), reference: PRESET_COMMIT.into(), sha256: Some(PRESET_SHA256.into()) });
    assert!(set.dir.join("tex/tudapub.cls").is_file() && set.kinds.len() == 11);
    // the same files as the archive of the test stack gives
    let cached = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tex-env/cache/tuda-ci.tar.gz")).ok();
    if let Some(c) = cached {
        assert_eq!(super::archive::sha256_hex(&c), PRESET_SHA256);
    }
    let _ = std::fs::remove_dir_all(&vault);
}

// spec: TEX-006
#[test]
#[ignore = "needs the network and the container: tests/tex-env/tex-env.sh build"]
fn tex_import_the_newest_tuda_ci_from_ctan_is_imported_and_its_test_page_compiles() {
    crate::env_lock::hold();
    let root = mounted_root();
    let vault = root.join(format!("vault-ctan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&vault);
    std::fs::create_dir_all(&vault).unwrap();
    let engine = container_engine(&root);
    let got = import(&request(&vault, What::Ctan, engine.clone()), &Handle::default(), &|| {}).unwrap_or_else(|e| panic!("the CTAN import failed: {e}"));
    assert_eq!((got.id.as_str(), got.kinds), ("tuda", 11));
    eprintln!("CTAN has tuda-ci {:?}", got.version);
    let set = scan(&vault).remove(0).info.unwrap();
    assert_eq!(set.meta.source, Source::Ctan { url: CTAN_URL.into() });
    assert_eq!(super::sets_ui::what_for(&set.meta.source), What::Ctan);
    let (id, kind) = test_kind(&set).unwrap();
    let out = compile_with_set(&engine, &set, "page.tex", &sample_document(kind, &set.settings, false), &[], patient()).unwrap_or_else(|e| panic!("kind {id}: {e}\n{}", e.log_tail));
    assert!(out.placeholder_logo && out.output.pdf.starts_with(b"%PDF"));
    let _ = std::fs::remove_dir_all(&vault);
}
