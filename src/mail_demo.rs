//! A small made-up mail store for screenshots (`--screenshot out.png --view mail`), like the
//! demo vault: no server and no real mail are involved.

use crate::mail_db::{FolderRow, Op};
use crate::mail_store::MailStore;
use std::path::Path;

/// The account id of the demo.
pub const ACCOUNT: &str = "demo";

fn eml(from: &str, subject: &str, day: u32, body: &str, attachment: bool) -> Vec<u8> {
    let head = format!("From: {from}\r\nTo: Ada Example <ada@example.com>\r\nSubject: {subject}\r\nDate: {day:02} Nov 2026 09:{:02}:00 +0100\r\nMessage-ID: <demo-{day}-{}@example.com>\r\nMIME-Version: 1.0\r\n", day * 3 % 60, subject.len());
    if attachment {
        format!("{head}Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n--b1\r\nContent-Type: application/pdf; name=\"floor-plan.pdf\"\r\nContent-Disposition: attachment; filename=\"floor-plan.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gUERG\r\n--b1--\r\n").into_bytes()
    } else {
        format!("{head}Content-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n").into_bytes()
    }
}

/// Fill the store at `root` with a few mails in an Inbox and a Sent folder.
pub fn seed(root: &Path) -> Result<(), String> {
    let store = MailStore::open(root).map_err(|e| e.0)?;
    let mails: &[(&str, &str, u32, &str, bool, bool)] = &[
        (
            "Hans Müller <hans@example.com>",
            "Lecture hall B: projector and floor plan",
            13,
            "Hello Ada,\r\n\r\nthe projector in hall B is booked for Thursday. The floor plan is attached; please check the seating before we confirm the room.\r\n\r\nBest,\r\nHans",
            true,
            false,
        ),
        ("Dora Berg <dora@example.com>", "Lunch on Friday?", 13, "Pizza at noon?\r\n\r\nDora", false, false),
        ("Bob Stone <bob@example.com>", "Invoice 2026-114", 12, "Please find the invoice for the lecture hall rental below. Payment is due within 30 days.\r\n\r\nBob", false, true),
        ("Carol Weber <carol@example.com>", "Re: Schedule for the autumn workshop", 11, "Thursday works for me. Shall we meet at 10:00 in room 2.14?\r\n\r\nCarol", false, true),
        ("Notes Newsletter <news@example.com>", "Your weekly digest", 10, "Five things that happened in notes this week.", false, true),
        ("Erik Ahlberg <erik@example.com>", "Budget questions", 9, "Could you send me the updated budget sheet before Monday?\r\n\r\nErik", false, true),
        ("Fatima Noor <fatima@example.com>", "Welcome to the project", 7, "Welcome aboard! The kick-off notes are in the shared folder.\r\n\r\nFatima", false, true),
        ("Gregor Lang <gregor@example.com>", "Parking permit", 5, "Your parking permit for November is ready at the front desk.", false, true),
    ];
    let inbox = store.ensure_folder(ACCOUNT, "Demo mail", "INBOX", Some("/"), None, true).map_err(|e| e.0)?;
    let sent = store.ensure_folder(ACCOUNT, "Demo mail", "Sent", Some("/"), Some("\\Sent"), true).map_err(|e| e.0)?;
    store.ensure_folder(ACCOUNT, "Demo mail", "Trash", Some("/"), Some("\\Trash"), false).map_err(|e| e.0)?;
    store.ensure_folder(ACCOUNT, "Demo mail", "Projects/Hall B", Some("/"), None, true).map_err(|e| e.0)?;
    let fill = |folder: &FolderRow, items: Vec<(Vec<u8>, bool, i64)>| -> Result<(), String> {
        let f = FolderRow { uidvalidity: Some(1), ..folder.clone() };
        let mut ops = vec![Op::FolderState { folder: f.id, uidvalidity: Some(1), uidnext: Some(items.len() as u32 + 1), highestmodseq: None, last_sync: 1 }];
        for (i, (raw, seen, date)) in items.iter().enumerate() {
            let m = store.save_message(&f, 1, i as u32 + 1, raw, if *seen { "\\Seen" } else { "" }, Some(*date)).map_err(|e| e.0)?;
            ops.push(Op::message(m));
        }
        store.commit(ops).map_err(|e| e.0)
    };
    // oldest first, so the newest has the highest UID
    let mut inbox_items: Vec<(Vec<u8>, bool, i64)> = mails.iter().rev().map(|(from, subject, day, body, att, seen)| (eml(from, subject, *day, body, *att), *seen, 1_794_000_000 + i64::from(*day) * 86_400)).collect();
    inbox_items.sort_by_key(|x| x.2);
    fill(&inbox, inbox_items)?;
    fill(&sent, vec![(eml("Ada Example <ada@example.com>", "Re: Schedule for the autumn workshop", 11, "Thursday is fine, see you there.\r\n\r\nAda", false), true, 1_794_000_000 + 11 * 86_400)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MAIL-050
    #[test]
    fn the_demo_store_has_mail_in_its_folders() {
        let root = std::env::temp_dir().join(format!("shale-mail-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        seed(&root).unwrap();
        let store = MailStore::open(&root).unwrap();
        let r = store.reader().unwrap();
        let folders = r.folders(Some(ACCOUNT)).unwrap();
        let inbox = folders.iter().find(|f| f.name == "INBOX").unwrap();
        let list = r.list(inbox.id, 50, 0).unwrap();
        assert_eq!(list.len(), 8);
        assert!(list[0].subject.starts_with("Lecture hall B") || list[0].subject.starts_with("Lunch"), "{:?}", list[0].subject);
        assert!(list.iter().any(|m| m.has_attachments));
        let _ = std::fs::remove_dir_all(&root);
    }
}
