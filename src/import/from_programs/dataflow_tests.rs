//! The dataflow diagram of a drop from another program (DEV-030, `docs/dataflow/import-programs.md`):
//! a list of files, a picture that is only offered as a web address (the download is a stand-in)
//! and plain text, run under the recorder.

use super::*;

// spec: DEV-030
#[test]
fn dev_030_a_drop_from_another_program_is_a_dataflow_diagram() {
    let dir = std::env::temp_dir().join(format!("shale-import-flow-programs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let eml = dir.join("message.eml");
    std::fs::write(&eml, crate::mail::tests::sample_eml()).unwrap();
    let files = vec![("text/uri-list".to_string(), format!("{}\r\n", file_uri(&eml)).into_bytes())];
    let address = vec![("text/uri-list".to_string(), b"https://example.org/img/cat.png\r\n".to_vec())];
    let text = vec![("text/plain".to_string(), b"Beamer tauschen".to_vec())];
    let download = |_: &str| Ok((b"PNG".to_vec(), Some("image/png".to_string())));
    let flow = crate::dataflow::record(|| {
        assert_eq!(items_from_parts_in(&files, &[], &download).0.len(), 1);
        assert_eq!(items_from_parts_in(&address, &[], &download).0.len(), 1);
        assert_eq!(items_from_parts_in(&text, &[], &download).0.len(), 1);
    });
    let doc = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/import-programs.md");
    crate::dataflow::verify(&doc, "import_programs", &crate::dataflow::render_mermaid(&flow));
    let _ = std::fs::remove_dir_all(&dir);
}
