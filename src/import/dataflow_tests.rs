//! The dataflow diagrams of the Word import and of an imported e-mail (DEV-030, `docs/dataflow/`):
//! each test runs the flow under the recorder and compares the diagram with the committed one.
//! The other two flows of the import (the Thunderbird store, a drop from another program) have
//! their tests next to the code they drive, because that code is private.

use super::*;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-import-flow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn doc(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow").join(name)
}

// spec: DEV-030
#[test]
fn dev_030_an_imported_email_is_a_dataflow_diagram() {
    let dir = temp_dir("mail");
    let mut vault = Vault::open(&dir.join("vault")).unwrap();
    let raw = crate::mail::tests::sample_eml();
    let items = vec![email_item(mail::parse(&raw), ("Neubau.eml".to_string(), raw))];
    let o = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: true, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
    let mut done = None;
    let flow = crate::dataflow::record(|| done = Some(import(&mut vault, &items, &o)));
    let done = done.expect("the import ran");
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    assert_eq!(done.notes.len(), 1);
    crate::dataflow::verify(&doc("import-mail-text.md"), "import_mail_text", &crate::dataflow::render_mermaid(&flow));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A stand-in for pandoc that follows its contract: a picture next to where it runs, the note to
/// the file after `-o`.
#[cfg(unix)]
const PANDOC: &str = r#"
while [ $# -gt 0 ]; do case "$1" in -o) out="$2"; shift;; --lua-filter=*) cat "${1#--lua-filter=}" > /dev/null;; -M) meta="$2"; shift;; esac; shift; done
mkdir -p media; printf 'PNG' > media/image1.png
printf '## Fake\n\n![[%s/media/image1.png]]\n' "${meta#shale-media=}" > "$out""#;

// spec: DEV-030
#[cfg(unix)]
#[test]
fn dev_030_a_word_import_is_a_dataflow_diagram() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("docx");
    let pandoc = dir.join("pandoc");
    crate::fsutil::write_atomic(&pandoc, format!("#!/bin/sh\n{PANDOC}\n").as_bytes()).unwrap();
    std::fs::set_permissions(&pandoc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let word = dir.join("Report.docx");
    crate::fsutil::create_new(&word, b"PK fake docx").unwrap();
    let mut vault = Vault::open(&dir.join("vault")).unwrap();
    let items = vec![docx::docx_item(word, Some(pandoc))];
    let o = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false };
    let mut done = None;
    let flow = crate::dataflow::record(|| done = Some(import(&mut vault, &items, &o)));
    let done = done.expect("the import ran");
    assert!(done.errors.is_empty(), "{:?}", done.errors);
    assert_eq!(done.notes.len(), 1);
    crate::dataflow::verify(&doc("import-docx.md"), "import_docx", &crate::dataflow::render_mermaid(&flow));
    let _ = std::fs::remove_dir_all(&dir);
}
