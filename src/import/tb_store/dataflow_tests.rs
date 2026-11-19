//! The dataflow diagram of the Thunderbird store (DEV-030, `docs/dataflow/import-thunderbird.md`):
//! three lookups against a generated profile (by the position in the link, by the folder's index,
//! by the subject after a stale position) run under the recorder.

use super::*;
use crate::import::tb_fixture_tests::{Kind, Msg, TbFixture};

// spec: DEV-030
#[test]
fn dev_030_a_thunderbird_lookup_is_a_dataflow_diagram() {
    let fx = TbFixture::new("dataflow");
    let local = fx.account("server1", Kind::Local, "Local Folders", "nobody", "Mail/Local Folders");
    let (_, at) = fx.mbox(&local, &["Inbox"], &[Msg::new("Erste", "eins"), Msg::new("Verschoben", "nach dem Kompaktieren")]);
    let imap = fx.account("server2", Kind::Imap, "imap.uni.example", "ada", "ImapMail/imap.uni.example");
    let (file, imap_at) = fx.mbox(&imap, &["INBOX", "Bau"], &[Msg::new("Plan", "plan").id("p@x")]);
    fx.index(&file, &[(40, "Plan", "p@x", &imap_at[0].to_string())]);
    let roots = std::slice::from_ref(&fx.root);
    let flow = crate::dataflow::record(|| {
        let by_position = find_in(roots, &[format!("mailbox-message://nobody@Local%20Folders/Inbox#{}", at[1])], None).unwrap();
        assert_eq!(by_position.len(), 1);
        let by_index = find_in(roots, &["imap-message://ada@imap.uni.example/INBOX/Bau#40".to_string()], None).unwrap();
        assert_eq!(by_index.len(), 1);
        let by_subject = find_in(roots, &["mailbox-message://nobody@Local%20Folders/Inbox#99999999".to_string()], Some("Verschoben.eml")).unwrap();
        assert_eq!(by_subject.len(), 1);
    });
    let doc = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/import-thunderbird.md");
    crate::dataflow::verify(&doc, "import_thunderbird", &crate::dataflow::render_mermaid(&flow));
}
