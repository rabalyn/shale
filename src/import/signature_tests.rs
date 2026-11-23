//! The signature attachments of an imported e-mail start unticked (IMP-060, IMP-061).

use super::*;
use crate::mail::crypto::testmail::{smime_signed, ENTITY};
use crate::mail::AttKind;

const PNG: &str = "iVBORw0KGgo=";

/// An S/MIME signed mail, a picture of the text, a logo of the signature and a real attachment.
fn mixed_mail() -> Vec<u8> {
    let mut m = String::from("From: Jane <j@x.org>\r\nTo: b@x.org\r\nSubject: Plan\r\nDate: Sat, 14 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: multipart/related; boundary=\"R\"\r\n\r\n--R\r\nContent-Type: text/html; charset=utf-8\r\n\r\n");
    m.push_str("<p>Hi</p><div class=\"moz-signature\">-- <br>Jane <img src=\"cid:logo@x\"></div>");
    m.push_str(&format!("\r\n--R\r\nContent-Type: image/png; name=\"firm.png\"\r\nContent-ID: <logo@x>\r\nContent-Disposition: attachment; filename=\"firm.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{PNG}\r\n--R--\r\n"));
    m.push_str(&format!("--M\r\nContent-Type: image/png; name=\"holiday.png\"\r\nContent-Disposition: attachment; filename=\"holiday.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{PNG}\r\n"));
    m.push_str("--M\r\nContent-Type: application/pkcs7-signature; name=smime.p7s\r\nContent-Transfer-Encoding: base64\r\n\r\nMIIBAgYJKoZI\r\n--M--\r\n");
    m.into_bytes()
}

// spec: IMP-060
#[test]
fn signature_parts_start_unticked_and_a_real_picture_stays_ticked() {
    let item = email_item(mail::parse(&mixed_mail()), ("m.eml".into(), vec![]));
    let Source::Email { mail, .. } = &item.source else { panic!() };
    let kinds: Vec<_> = mail.attachments.iter().map(|a| (a.name.as_str(), a.kind)).collect();
    assert_eq!(kinds, vec![("firm.png", AttKind::SignatureImage), ("holiday.png", AttKind::Content), ("smime.p7s", AttKind::CryptoSignature)]);
    assert_eq!(item.att_on, vec![false, true, false]);
    // the crypto parts of an S/MIME mail made like the mail view's tests do
    let signed = email_item(mail::parse(&smime_signed(&[0x30, 0x82, 1], ENTITY)), ("s.eml".into(), vec![]));
    assert_eq!(signed.att_on, vec![false]);
}

// spec: IMP-061
#[test]
fn skip_signatures_off_ticks_the_signature_parts_and_on_unticks_them_again() {
    let mut items = vec![email_item(mail::parse(&mixed_mail()), ("m.eml".into(), vec![]))];
    items[0].att_on[1] = false; // the user left out the real picture
    apply_skip_signatures(&mut items, false);
    assert_eq!(items[0].att_on, vec![true, false, true], "the signature parts are ticked, the user's choice for the others stays");
    apply_skip_signatures(&mut items, true);
    assert_eq!(items[0].att_on, vec![false, false, false]);
}

// spec: IMP-061
#[test]
fn skip_signatures_off_keeps_the_rule_for_small_inline_pictures() {
    let a = crate::mail::Attachment { name: "logo.png".into(), mime: "image/png".into(), data: vec![0; 100], inline: true, cid: "l".into(), kind: AttKind::SignatureImage };
    assert!(!attachment_starts_on(&a, true));
    assert!(!attachment_starts_on(&a, false), "a small inline picture stays out, as before");
    let big = crate::mail::Attachment { data: vec![0; 40_000], ..a };
    assert!(attachment_starts_on(&big, false), "a big one is ticked");
    assert!(!attachment_starts_on(&big, true));
}

// spec: IMP-060
#[test]
fn an_import_brings_only_the_ticked_attachments() {
    let dir = std::env::temp_dir().join(format!("shale-import-sig-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut v = Vault::open(&dir).unwrap();
    let item = email_item(mail::parse(&mixed_mail()), ("m.eml".into(), mixed_mail()));
    let o = Options { folder: PathBuf::from("In"), tags: vec![], keep_original: true, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false, cut: Default::default() };
    let Imported { notes, errors, .. } = import(&mut v, &[item], &o);
    assert!(errors.is_empty(), "{errors:?}");
    let text = std::fs::read_to_string(v.abs(&notes[0])).unwrap();
    assert!(text.contains("holiday.png") && !text.contains("firm.png") && !text.contains("smime.p7s"), "{text}");
    assert!(text.contains(".eml"), "the original keeps everything: {text}");
    let _ = std::fs::remove_dir_all(&v.root);
}

/// A reply of an HTML mail with a signature of its own and a quote that has one, too.
fn thread_mail() -> Vec<u8> {
    b"From: Jane <j@x.org>\r\nTo: b@x.org\r\nSubject: Plan\r\nDate: Sat, 14 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<div>Friday is fine.</div><div class=\"moz-signature\">Jane Doe<br>Acme GmbH</div><div class=\"moz-cite-prefix\">On 12.11.26 09:15, Bob wrote:<br></div><blockquote type=\"cite\"><p>Can we meet?</p><div class=\"moz-signature\">-- <br>Bob<br>Tel 0301234567</div></blockquote>".to_vec()
}

// spec: IMP-065, IMP-066
#[test]
fn imp_065_an_import_cuts_the_signatures_and_the_quoted_mails_the_options_ask_for() {
    let dir = std::env::temp_dir().join(format!("shale-import-cut-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut v = Vault::open(&dir).unwrap();
    let mut message = |cut: mail_segments::TextCut, folder: &str| {
        let item = email_item(mail::parse(&thread_mail()), ("m.eml".into(), vec![]));
        let o = Options { folder: PathBuf::from(folder), tags: vec![], keep_original: false, keep_docx: true, people_dir: "Adressbuch".into(), header_list: false, cut };
        let Imported { notes, errors, .. } = import(&mut v, &[item], &o);
        assert!(errors.is_empty(), "{errors:?}");
        std::fs::read_to_string(v.abs(&notes[0])).unwrap()
    };
    let all = message(mail_segments::TextCut::default(), "A");
    assert!(all.contains("Acme GmbH") && all.contains("Tel 0301234567") && all.contains("> Can we meet?"), "off: the text as it is: {all}");
    let skip = message(mail_segments::TextCut { skip_signatures: true, leave_out_quoted: false }, "B");
    assert!(skip.contains("Friday is fine.") && skip.contains("On 12.11.26 09:15, Bob wrote:") && skip.contains("> Can we meet?"), "own text and quote stay: {skip}");
    assert!(!skip.contains("Acme GmbH") && !skip.contains("Tel 0301234567") && !skip.contains("Jane Doe"), "both signatures go: {skip}");
    let quotes = message(mail_segments::TextCut { skip_signatures: false, leave_out_quoted: true }, "C");
    assert!(quotes.contains("Friday is fine.") && quotes.contains("Acme GmbH") && !quotes.contains("Can we meet?") && !quotes.contains("wrote:"), "{quotes}");
    let _ = std::fs::remove_dir_all(&v.root);
}
