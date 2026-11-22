//! Signature attachments of parsed mail (IMP-059).

use super::crypto::testmail::{mail_of, smime_signed, ENTITY};
use super::{parse, AttKind};

const PNG: &str = "iVBORw0KGgo=";

fn kinds(raw: &[u8]) -> Vec<(String, AttKind)> {
    parse(raw).attachments.into_iter().map(|a| (a.name, a.kind)).collect()
}

fn related(html: &str, parts: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut m = String::from("From: a@x.org\r\nSubject: s\r\nMIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"R\"\r\n\r\n--R\r\nContent-Type: text/html; charset=utf-8\r\n\r\n");
    m.push_str(html);
    for (name, cid, disp) in parts {
        let cid = if cid.is_empty() { String::new() } else { format!("Content-ID: <{cid}>\r\n") };
        m.push_str(&format!("\r\n--R\r\nContent-Type: image/png; name=\"{name}\"\r\n{cid}Content-Disposition: {disp}; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{PNG}"));
    }
    m.push_str("\r\n--R--\r\n");
    m.into_bytes()
}

// spec: IMP-059
#[test]
fn the_crypto_parts_of_signed_mail_are_signature_attachments() {
    let smime = smime_signed(&[0x30, 0x82, 1, 2, 3], ENTITY);
    assert_eq!(kinds(&smime), vec![("smime.p7s".to_string(), AttKind::CryptoSignature)], "S/MIME, by content type");
    let mut pgp = mail_of("Content-Type: multipart/signed; micalg=pgp-sha256; protocol=\"application/pgp-signature\"; boundary=\"SB\"\r\n\r\n--SB\r\n");
    pgp.extend_from_slice(ENTITY);
    pgp.extend_from_slice(b"\r\n--SB\r\nContent-Type: application/pgp-signature; name=\"signature.asc\"\r\n\r\n-----BEGIN PGP SIGNATURE-----\r\n\r\nabc\r\n-----END PGP SIGNATURE-----\r\n\r\n--SB--\r\n");
    assert_eq!(kinds(&pgp), vec![("signature.asc".to_string(), AttKind::CryptoSignature)], "OpenPGP");
    // a part without the content type, known by its name
    let named = b"From: a@x.org\r\nSubject: s\r\nContent-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: text/plain\r\n\r\nhi\r\n--M\r\nContent-Type: application/octet-stream; name=\"cert.p7s\"\r\n\r\nxyz\r\n--M\r\nContent-Type: application/pdf; name=\"plan.pdf\"\r\n\r\nxyz\r\n--M--\r\n";
    assert_eq!(kinds(named), vec![("cert.p7s".to_string(), AttKind::CryptoSignature), ("plan.pdf".to_string(), AttKind::Content)]);
}

// spec: IMP-059
#[test]
fn a_thunderbird_logo_after_the_moz_signature_is_a_signature_picture_and_a_body_picture_is_not() {
    let html = "<p>See the plan:</p><img src=\"cid:plan@x\"><div class=\"moz-signature\">-- <br>Jane<br><img src=\"cid:logo@x\"></div>";
    let raw = related(html, &[("plan.png", "plan@x", "inline"), ("firm.png", "logo@x", "inline")]);
    assert_eq!(kinds(&raw), vec![("plan.png".to_string(), AttKind::Content), ("firm.png".to_string(), AttKind::SignatureImage)]);
}

// spec: IMP-059
#[test]
fn pictures_after_the_dash_delimiter_or_a_gmail_container_are_signature_pictures() {
    let html = "<div>Text <img src=\"cid:a\"></div><div>-- <br></div><div><img src=\"cid:b\"></div>";
    let raw = related(html, &[("a.png", "a", "inline"), ("b.png", "b", "inline")]);
    assert_eq!(kinds(&raw)[1].1, AttKind::SignatureImage, "after the delimiter line");
    assert_eq!(kinds(&raw)[0].1, AttKind::Content, "before it");
    let html = "<div>Text</div><div data-smartmail=\"gmail_signature\"><img src=\"CID:b\"></div>";
    let raw = related(html, &[("b.png", "b", "inline")]);
    assert_eq!(kinds(&raw)[0].1, AttKind::SignatureImage, "Gmail, the cid in any case");
    // a picture named both in the text and in the signature is part of the text
    let html = "<img src=\"cid:b\"><div id=\"Signature\"><img src=\"cid:b\"></div>";
    let raw = related(html, &[("b.png", "b", "inline")]);
    assert_eq!(kinds(&raw)[0].1, AttKind::Content);
}

// spec: IMP-059
#[test]
fn an_outlook_mail_with_image001_and_social_icons_has_signature_pictures() {
    let html = "<p>Hello</p><p><img src=\"cid:i1\"></p>";
    let raw = related(html, &[("image001.png", "i1", "inline"), ("linkedin.png", "", "attachment"), ("Facebook_icon.png", "", "attachment"), ("XING.gif", "", "attachment")]);
    let k = kinds(&raw);
    assert!(k.iter().all(|(_, k)| *k == AttKind::SignatureImage), "{k:?}");
}

// spec: IMP-059
#[test]
fn a_real_picture_attachment_stays_content_and_so_does_a_big_logo_named_picture() {
    let html = "<p>The photo is attached.</p>";
    let raw = related(html, &[("holiday.png", "", "attachment"), ("Grundriss.jpg", "", "attachment")]);
    assert!(kinds(&raw).iter().all(|(_, k)| *k == AttKind::Content), "{:?}", kinds(&raw));
    // a big picture with a typical name is content: the rule is for small pictures
    let big = "x".repeat(60_000);
    let mut m = String::from("From: a@x.org\r\nSubject: s\r\nContent-Type: multipart/mixed; boundary=\"M\"\r\n\r\n--M\r\nContent-Type: text/plain\r\n\r\nhi\r\n--M\r\nContent-Type: image/png; name=\"logo-neu.png\"\r\n\r\n");
    m.push_str(&big);
    m.push_str("\r\n--M--\r\n");
    assert_eq!(kinds(m.as_bytes()), vec![("logo-neu.png".to_string(), AttKind::Content)]);
}
