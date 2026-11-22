//! Which attachments of a message belong to its signature (IMP-059): the crypto signature parts
//! of a signed mail and the pictures of a signature block. The Import dialog leaves them out by
//! default (IMP-060).

use super::{AttKind, Attachment};

/// A picture under this size with a typical name is taken for part of a signature.
pub const SMALL_PICTURE: usize = 30_000;

/// Names of the pictures that mail programs and signatures are full of.
const SIGNATURE_PICTURE_NAMES: [&str; 8] = ["logo", "linkedin", "facebook", "twitter", "xing", "instagram", "youtube", "banner"];

/// What a signature part is called in the dialog: `None` for any other attachment.
pub fn reason(a: &Attachment) -> Option<&'static str> {
    match a.kind {
        AttKind::Content => None,
        AttKind::SignatureImage => Some("signature picture"),
        AttKind::CryptoSignature => Some(if is_pgp(&a.mime, &a.name) { "OpenPGP signature" } else { "S/MIME signature" }),
    }
}

fn is_pgp(mime: &str, name: &str) -> bool {
    mime.eq_ignore_ascii_case("application/pgp-signature") || name.eq_ignore_ascii_case("signature.asc")
}

/// A crypto signature part of a `multipart/signed` mail, by content type or file name.
pub fn is_crypto_signature(mime: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    is_pgp(mime, &name) || matches!(mime.to_ascii_lowercase().as_str(), "application/pkcs7-signature" | "application/x-pkcs7-signature") || name == "smime.p7s" || name.ends_with(".p7s")
}

fn is_picture(a: &Attachment) -> bool {
    a.mime.to_ascii_lowercase().starts_with("image/") || crate::media::is_image(std::path::Path::new(&a.name))
}

/// `image001.png` (Outlook) or a name that starts with a word of `SIGNATURE_PICTURE_NAMES`.
fn typical_name(name: &str) -> bool {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_ascii_lowercase();
    let outlook = stem.strip_prefix("image").is_some_and(|n| n.len() == 3 && n.bytes().all(|b| b.is_ascii_digit()));
    outlook || SIGNATURE_PICTURE_NAMES.iter().any(|w| stem.starts_with(w))
}

/// Where the signature of an HTML message starts: the first of a signature container of Thunderbird,
/// Gmail or Outlook and the `-- ` delimiter line.
fn signature_start(html: &str) -> Option<usize> {
    let low = html.to_ascii_lowercase();
    let containers = ["moz-signature", "gmail_signature", "id=\"signature\"", "id=\"x_signature\"", "class=\"signature\"", "mobile-signature"];
    let mut best = containers.iter().filter_map(|m| low.find(m)).min();
    for (i, _) in low.match_indices("--") {
        let before_ok = i == 0 || matches!(low.as_bytes()[i - 1], b'>' | b'\n' | b'\r' | b' ');
        let rest = &low[i + 2..];
        let rest = rest.strip_prefix(' ').or_else(|| rest.strip_prefix("&nbsp;"));
        let ends_line = rest.is_some_and(|r| {
            let r = r.trim_start_matches([' ', '\t']);
            r.is_empty() || r.starts_with(['\n', '\r']) || r.starts_with("<br") || r.starts_with("</")
        });
        if before_ok && ends_line {
            best = Some(best.map_or(i, |b| b.min(i)));
            break;
        }
    }
    best
}

/// The `cid:` references of the HTML with their positions, lowercase.
fn cid_refs(html: &str) -> Vec<(String, usize)> {
    let low = html.to_ascii_lowercase();
    low.match_indices("cid:")
        .map(|(i, _)| {
            let id: String = low[i + 4..].chars().take_while(|c| !matches!(c, '"' | '\'' | ')' | '>' | ' ' | '\n' | '\r' | '<')).collect();
            (id, i)
        })
        .collect()
}

/// Sets the kind of every attachment: crypto parts by type or name, pictures by the HTML (only
/// referenced after the signature starts) or by a typical name when small. `html` is the HTML
/// part of the message, if it has one.
pub fn classify(attachments: &mut [Attachment], html: Option<&str>) {
    let start = html.and_then(signature_start);
    let refs = html.map(cid_refs).unwrap_or_default();
    for a in attachments {
        if is_crypto_signature(&a.mime, &a.name) {
            a.kind = AttKind::CryptoSignature;
            continue;
        }
        if !is_picture(a) {
            continue;
        }
        let cid = a.cid.to_ascii_lowercase();
        let mine: Vec<usize> = refs.iter().filter(|(id, _)| !cid.is_empty() && *id == cid).map(|r| r.1).collect();
        let in_signature = start.is_some_and(|s| !mine.is_empty() && mine.iter().all(|p| *p >= s));
        if in_signature || (a.data.len() < SMALL_PICTURE && typical_name(&a.name)) {
            a.kind = AttKind::SignatureImage;
        }
    }
}
