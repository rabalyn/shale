//! A developer tool, not a test of a rule (DEV-024): `scripts/compare-docx-pdf.sh` runs this ignored
//! test to make the PDF Shale prints from a Word file (or from a note) without a window. The
//! environment says what to do: `SHALE_COMPARE_OUT` is the PDF to write and one of
//! `SHALE_COMPARE_DOCX` (a Word file, imported with the real pandoc as the import dialog does) or
//! `SHALE_COMPARE_MD` (a note, copied into a new vault) names the input. The export uses the options
//! the export dialog opens with for that note.

use crate::import::docx;
use crate::import::Options;
use crate::pdf::export::{export_note, ExportOptions};
use crate::vault::Vault;
use std::path::PathBuf;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[test]
#[ignore = "run by scripts/compare-docx-pdf.sh"]
fn dev_024_compare_export_writes_the_pdf_of_a_docx_or_a_note() {
    let out = env_path("SHALE_COMPARE_OUT").expect("SHALE_COMPARE_OUT is the PDF to write");
    let dir = std::env::temp_dir().join(format!("shale-compare-vault-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut vault = Vault::open(&dir).unwrap();
    let rel = if let Some(md) = env_path("SHALE_COMPARE_MD") {
        let name = md.file_name().expect("SHALE_COMPARE_MD names a file").to_string_lossy().into_owned();
        vault.create_file(std::path::Path::new(&name), &std::fs::read_to_string(&md).unwrap()).unwrap()
    } else {
        let word = env_path("SHALE_COMPARE_DOCX").expect("SHALE_COMPARE_DOCX or SHALE_COMPARE_MD names the input");
        let pandoc = docx::locate("").expect("pandoc is not installed");
        let item = docx::docx_item(word, Some(pandoc));
        let opts = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: false, keep_docx: false, people_dir: "Adressbuch".into() };
        docx::import_item(&mut vault, &item, &opts).unwrap()
    };
    let text = std::fs::read_to_string(vault.root.join(&rel)).unwrap();
    let opts = ExportOptions::for_note().with_note_properties(&text);
    let pdf = export_note(&vault, &rel, &text, &opts).unwrap();
    std::fs::write(&out, pdf).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
