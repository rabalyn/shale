//! The guided setup of the LaTeX export against the real LuaLaTeX of the container
//! (`tests/tex-env/`, ignored unless asked for: `cargo test -- --ignored tex_env`): the functions of
//! its steps, without a window, from the import of the cached tuda-ci archive to a test page that
//! is a PDF with the person's department on it (TEX-017).

use super::compile_tests::container_engine;
use super::engine::{Detection, Engine};
use super::import::{import, What};
use super::install::Handle;
use super::pdf_text::{page_texts, squashed};
use super::sets::{compile_with_set, Language, LogoStatus};
use super::sets_env_tests::{cached_archive, mounted_root, patient, request};
use super::sets_ui::VaultRef;
use super::settings_ui::TexState;
use super::setup::*;
use crate::gitsync::TreeLock;
use crate::vault::Vault;

fn detection(engine: Engine) -> Detection {
    Detection { system: Ok(engine), managed: Err("not installed".into()), managed_bytes: None }
}

// spec: TEX-017, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_the_setup_imports_the_cached_archive_takes_a_logo_and_the_details_and_makes_a_test_page_with_the_department() {
    let root = mounted_root();
    let dir = root.join(format!("vault-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let engine = container_engine(&root);
    // the templates step: the cached archive, as a file
    let got = import(&request(&dir, What::Local(cached_archive()), engine.clone()), &Handle::default(), &|| {}).unwrap_or_else(|e| panic!("the import failed: {e}"));
    assert_eq!((got.id.as_str(), got.kinds), ("tuda", 11));
    // the state of the window, as the setup sees it
    let mut st = TexState::new(false, "setup-env");
    st.detection = Some(detection(engine.clone()));
    st.sets.bind(Some(VaultRef { root: dir.clone(), tree: TreeLock::default() }));
    st.sets.refresh();
    let cfg = super::engine::TexConfig::default();
    st.open_setup(&cfg, None, Return::Nothing);
    assert_eq!(st.setup.step, Step::Logo, "TeX and templates are done, the logo is not");
    assert!(st.step_done(&cfg, Step::Tex) && st.step_done(&cfg, Step::Templates) && !st.step_done(&cfg, Step::Logo));
    // the logo step: a file that is not the placeholder
    let logo = root.join(format!("logo-{}.pdf", std::process::id()));
    std::fs::write(&logo, String::from_utf8(super::sets::placeholder_logo_pdf()).unwrap().replace("0.75 g", "0.20 g")).unwrap();
    st.setup_logo(&logo);
    assert!(st.setup.message.as_ref().is_some_and(|n| n.ok), "{:?}", st.setup.message);
    assert_eq!(st.chosen_set().unwrap().logo, LogoStatus::Present("tuda_logo.pdf".into()));
    // the details step: the accent colours of the set, then the choices
    st.setup_go(Step::Details);
    assert_eq!(st.setup.accents.len(), 48, "every colour of tudacolors.def: 0a to 0d and 1a to 11d");
    assert_eq!(st.setup.accents.iter().find(|s| s.code == "9c").map(|s| s.rgb), Some([185, 15, 34]));
    st.setup.details = Details { accent: "9c".into(), language: Language::De, department: "Fachbereich Informatik".into(), author: "Ada Lovelace".into() };
    st.setup_save_details().unwrap();
    let set = st.chosen_set().unwrap().clone();
    assert_eq!((set.settings.accent.as_str(), set.settings.language, set.settings.department.as_str()), ("9c", Language::De, "Fachbereich Informatik"));
    // the test page: through the writer, compiled in the container
    let vault = Vault::open(&dir).unwrap();
    let (kind, doc) = test_page(&vault, &set).unwrap();
    assert_eq!(kind, "thesis", "a department is shown by the thesis");
    let files: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &files, patient()).unwrap_or_else(|e| panic!("the test page failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    assert!(!out.placeholder_logo, "the logo is the one that was chosen");
    let pages = page_texts(out.output.pdf.clone());
    assert!(!pages.is_empty(), "the PDF has pages");
    let text = squashed(out.output.pdf);
    let has = |needle: &str| text.contains(&needle.chars().filter(|c| !c.is_whitespace()).collect::<String>());
    assert!(has("Fachbereich Informatik"), "the department is on the page:\n{text}\n--- main.tex:\n{}", doc.tex);
    assert!(has("Ada Lovelace") && has("Shale test page"), "the author and the title are too:\n{text}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&logo);
}
