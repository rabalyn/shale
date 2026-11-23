//! The signature of a release file (UPD-022, ADR-0037): an SSH signature (`ssh-keygen -Y sign`,
//! the SSHSIG format of OpenSSH's PROTOCOL.sshsig) made with an ed25519 key whose public half is
//! built into Shale. The owner signs, Shale only verifies, with `ring` (no other crypto crate).
//! Verification checks the magic and version, that the key in the signature is the built-in one,
//! the namespace, the hash algorithm (`sha512` only) and then the ed25519 signature over the data
//! that PROTOCOL.sshsig defines, whose hash is made from the file in chunks.

use ring::digest::{self, Context, SHA512};
use ring::signature::{UnparsedPublicKey, ED25519};
use std::io::Read;
use std::path::Path;

/// The public key of the release signing key, as `ssh-keygen` writes it (`ssh-ed25519 AAAA… comment`).
/// The same file is what a person puts into an `allowed_signers` file to check a download by hand.
const RELEASE_KEY: &str = include_str!("release-signing.pub");

/// The most bytes of a signature file that are read.
pub const MAX_SIG: u64 = 16 << 10;

const ARMOR_BEGIN: &str = "-----BEGIN SSH SIGNATURE-----";
const ARMOR_END: &str = "-----END SSH SIGNATURE-----";
const MAGIC: &[u8] = b"SSHSIG";
const KEY_TYPE: &[u8] = b"ssh-ed25519";

/// The namespace a release file is signed in: it names the file (so also its version and
/// system), so a signature of another file of the owner, or of an older version, does not fit.
pub fn namespace(asset: &str) -> String {
    format!("shale-release/{asset}")
}

/// The 32 bytes of the key built into Shale.
pub fn release_key() -> [u8; 32] {
    public_key(RELEASE_KEY).expect("the key built into Shale (src/update/release-signing.pub) is an OpenSSH ed25519 public key")
}

/// A reader of the SSH wire format: `string` is a big-endian length and that many bytes.
struct Wire<'a>(&'a [u8]);

impl<'a> Wire<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.0.len() < n {
            return Err("it is cut short".into());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<&'a [u8], String> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    fn done(&self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("it has more data than it should".into())
        }
    }
}

/// `string` in the wire format.
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// The bytes of standard base64 (padding allowed, nothing else but the alphabet).
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The 32 key bytes in an `ssh-ed25519 AAAA… comment` line (the wire format inside: the type,
/// then the key).
pub fn public_key(openssh: &str) -> Result<[u8; 32], String> {
    let mut words = openssh.split_whitespace();
    if words.next() != Some("ssh-ed25519") {
        return Err("the public key is not an ssh-ed25519 key".into());
    }
    let blob = words.next().and_then(base64).ok_or("the public key is not base64")?;
    key_blob(&blob)
}

/// The key in the wire format of a public key (`string "ssh-ed25519"`, `string key`).
fn key_blob(blob: &[u8]) -> Result<[u8; 32], String> {
    let mut w = Wire(blob);
    let kind = w.string().map_err(|e| format!("the public key: {e}"))?;
    if kind != KEY_TYPE {
        return Err("the public key is not an ssh-ed25519 key".into());
    }
    let key = w.string().map_err(|e| format!("the public key: {e}"))?;
    w.done().map_err(|e| format!("the public key: {e}"))?;
    key.try_into().map_err(|_| "the public key is not 32 bytes long".to_string())
}

/// The blob inside an armored signature (`-----BEGIN SSH SIGNATURE-----` … `-----END SSH SIGNATURE-----`).
fn unarmor(text: &str) -> Result<Vec<u8>, String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    match (lines.first(), lines.last()) {
        (Some(&ARMOR_BEGIN), Some(&ARMOR_END)) if lines.len() > 2 => base64(&lines[1..lines.len() - 1].concat()).ok_or_else(|| "it is not base64".into()),
        _ => Err("it is not an SSH signature (no BEGIN and END line)".into()),
    }
}

/// The SHA-512 of a file, read in chunks.
fn hash_file(path: &Path) -> Result<digest::Digest, String> {
    let mut file = crate::fsutil::open_read(path).map_err(|e| format!("the file could not be read ({e})"))?;
    let mut context = Context::new(&SHA512);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(context.finish()),
            Ok(n) => context.update(&buf[..n]),
            Err(e) => return Err(format!("the file could not be read ({e})")),
        }
    }
}

/// Whether `signature` (the text of a `.sig` file) is the signature of the file at `path` that
/// the owner of `key` made in `namespace`. The error says in words what is wrong.
pub fn verify_file(path: &Path, signature: &str, key: &[u8; 32], namespace: &str) -> Result<(), String> {
    let blob = unarmor(signature).map_err(|e| format!("the signature file is damaged: {e}"))?;
    let bad = |e: String| format!("the signature file is damaged: {e}");
    let mut w = Wire(&blob);
    if w.take(MAGIC.len()).map_err(bad)? != MAGIC {
        return Err("the signature file is not an SSH signature (wrong magic)".into());
    }
    let version = w.u32().map_err(bad)?;
    if version != 1 {
        return Err(format!("the signature has version {version}, which Shale does not know"));
    }
    let signer = key_blob(w.string().map_err(bad)?).map_err(|e| format!("the signature file is damaged: {e}"))?;
    let signed_in = w.string().map_err(bad)?;
    let reserved = w.string().map_err(bad)?;
    let algorithm = w.string().map_err(bad)?;
    let sig_blob = w.string().map_err(bad)?;
    w.done().map_err(bad)?;
    let mut s = Wire(sig_blob);
    let (sig_kind, sig) = (s.string().map_err(bad)?, s.string().map_err(bad)?);
    s.done().map_err(bad)?;
    if &signer != key {
        return Err("it was not signed with the release key built into Shale".into());
    }
    if signed_in != namespace.as_bytes() {
        return Err(format!("it was signed for another file than {namespace:?}"));
    }
    if algorithm != b"sha512" {
        return Err("it uses a hash that Shale does not accept (only sha512)".into());
    }
    if sig_kind != KEY_TYPE {
        return Err("the signature is not an ssh-ed25519 signature".into());
    }
    let hash = hash_file(path)?;
    let mut data = MAGIC.to_vec();
    put(&mut data, signed_in);
    put(&mut data, reserved);
    put(&mut data, algorithm);
    put(&mut data, hash.as_ref());
    UnparsedPublicKey::new(&ED25519, key).verify(&data, sig).map_err(|_| "the signature does not match the file".to_string())
}

/// Signing for tests: the same format `ssh-keygen -Y sign` writes, from a seed.
#[cfg(test)]
pub mod testing {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// The key pair of a test seed.
    pub fn pair(seed: u8) -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
    }

    /// The 32 public key bytes of a test seed.
    pub fn public(seed: u8) -> [u8; 32] {
        pair(seed).public_key().as_ref().try_into().unwrap()
    }

    fn armor(blob: &[u8]) -> String {
        let text = crate::mail::imap_client::base64_encode(blob);
        let lines: Vec<&str> = text.as_bytes().chunks(70).map(|c| std::str::from_utf8(c).unwrap()).collect();
        format!("{ARMOR_BEGIN}\n{}\n{ARMOR_END}\n", lines.join("\n"))
    }

    /// The signature file of `data` in `namespace`, signed with the test key `seed`.
    pub fn sign(seed: u8, namespace: &str, data: &[u8]) -> String {
        let pair = pair(seed);
        let mut key = vec![];
        put(&mut key, KEY_TYPE);
        put(&mut key, pair.public_key().as_ref());
        let mut signed = MAGIC.to_vec();
        put(&mut signed, namespace.as_bytes());
        put(&mut signed, b"");
        put(&mut signed, b"sha512");
        put(&mut signed, digest::digest(&SHA512, data).as_ref());
        let mut sig = vec![];
        put(&mut sig, KEY_TYPE);
        put(&mut sig, pair.sign(&signed).as_ref());
        let mut blob = MAGIC.to_vec();
        blob.extend_from_slice(&1u32.to_be_bytes());
        put(&mut blob, &key);
        put(&mut blob, namespace.as_bytes());
        put(&mut blob, b"");
        put(&mut blob, b"sha512");
        put(&mut blob, &sig);
        armor(&blob)
    }

    /// Like [`sign`] but a blob that `edit` has changed before it is armored (tests of damage).
    pub fn sign_edited(seed: u8, namespace: &str, data: &[u8], edit: impl FnOnce(&mut Vec<u8>)) -> String {
        let text = sign(seed, namespace, data);
        let mut blob = unarmor(&text).unwrap();
        edit(&mut blob);
        armor(&blob)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{public, sign, sign_edited};
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("shale-sshsig-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    // spec: UPD-022
    #[test]
    fn upd_022_the_key_built_into_shale_is_the_one_of_the_published_public_key_file() {
        let key = release_key();
        assert_eq!(key, public_key(include_str!("release-signing.pub")).unwrap());
        assert_ne!(key, [0u8; 32]);
        // the same file is what the repository publishes for a check by hand
        assert!(RELEASE_KEY.starts_with("ssh-ed25519 AAAA"));
    }

    // spec: UPD-022
    #[test]
    fn upd_022_an_openssh_public_key_is_read_and_a_wrong_one_is_refused() {
        // made by `ssh-keygen -t ed25519`; the 32 bytes after the type
        let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAb61OywJxPunQYR/F8tjG1h8RjMk5GZWZOJnmDea0uz some comment";
        let key = public_key(line).unwrap();
        assert_eq!(key[..4], [0x06, 0xfa, 0xd4, 0xec]);
        assert_eq!(public_key(&line.replace("some comment", "")).unwrap(), key, "the comment is optional");
        for (text, why) in [
            ("ssh-rsa AAAAB3NzaC1yc2E= x", "not an ssh-ed25519"),
            ("", "not an ssh-ed25519"),
            ("ssh-ed25519", "not base64"),
            ("ssh-ed25519 ****", "not base64"),
            ("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5", "cut short"),
            // the type of a rsa key in the blob of an ed25519 line
            ("ssh-ed25519 AAAAB3NzaC1yc2EAAAAgBvrUXTrsXo8LTIYHYuAJvQ2q", "not an ssh-ed25519"),
        ] {
            let e = public_key(text).unwrap_err();
            assert!(e.contains(why), "{text:?}: {e}");
        }
        // the wrong length of the key
        let mut blob = vec![];
        put(&mut blob, KEY_TYPE);
        put(&mut blob, &[1u8; 31]);
        assert!(key_blob(&blob).unwrap_err().contains("32 bytes"));
        blob.push(0);
        assert!(key_blob(&blob).unwrap_err().contains("more data"));
    }

    // spec: UPD-022
    #[test]
    fn upd_022_base64_is_read_with_and_without_padding_and_nothing_else() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64("").unwrap(), b"");
        assert!(base64("aGVs bG8=").is_none(), "a space is not base64");
        assert!(base64("aGVs-bG8").is_none(), "the url alphabet is not accepted");
    }

    // spec: UPD-022
    #[test]
    fn upd_022_a_signature_of_the_file_in_its_namespace_by_the_key_is_valid() {
        let path = file("valid", b"the release file");
        let ns = namespace("shale-1.0.0-linux-x86_64.tar.gz");
        let sig = sign(7, &ns, b"the release file");
        verify_file(&path, &sig, &public(7), &ns).unwrap();
        // the armor may have other line breaks and a final newline or not
        verify_file(&path, sig.trim_end(), &public(7), &ns).unwrap();
        verify_file(&path, &sig.replace('\n', "\r\n"), &public(7), &ns).unwrap();
        assert_eq!(ns, "shale-release/shale-1.0.0-linux-x86_64.tar.gz");
    }

    // A fixed vector: a signature that `ssh-keygen -Y sign` wrote (OpenSSH 10.6) for the file
    // "hello\n" with a throwaway key, in the namespace of a release file.
    // spec: UPD-022
    #[test]
    fn upd_022_a_fixed_signature_of_ssh_keygen_is_valid_and_fails_for_any_other_input() {
        const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICRDd7wUiS18eq9OMqdtR54V/AlgpaQeoKjqze4TmY5h test";
        const SIG: &str = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgJEN3vBSJLXx6r04yp21HnhX8CW\nClpB6gqOrN7hOZjmEAAAAtc2hhbGUtcmVsZWFzZS9zaGFsZS0wLjcuMC1saW51eC14ODZf\nNjQudGFyLmd6AAAAAAAAAAZzaGE1MTIAAABTAAAAC3NzaC1lZDI1NTE5AAAAQHczt5W1rC\n5p2Jfn+bIj3fzZbr8iagzxvsvJ+S76Yn2wUGTJj3y8oA/nRPTfECBOCSi5DvPRNHEkBUY9\n5FejZwA=\n-----END SSH SIGNATURE-----\n";
        const NS: &str = "shale-release/shale-0.7.0-linux-x86_64.tar.gz";
        let key = public_key(KEY).unwrap();
        verify_file(&file("vector", b"hello\n"), SIG, &key, NS).unwrap();
        assert!(verify_file(&file("vector-changed", b"hello"), SIG, &key, NS).is_err());
        assert!(verify_file(&file("vector-ns", b"hello\n"), SIG, &key, "shale-release/shale-0.7.1-linux-x86_64.tar.gz").is_err());
        assert!(verify_file(&file("vector-key", b"hello\n"), SIG, &release_key(), NS).is_err(), "not the key of the release");
    }

    // spec: UPD-022
    #[test]
    fn upd_022_a_file_that_is_bigger_than_a_chunk_is_hashed_whole() {
        let big = vec![0xA5u8; (3 << 20) + 17];
        let path = file("big", &big);
        verify_file(&path, &sign(7, "ns", &big), &public(7), "ns").unwrap();
    }

    // spec: UPD-022
    #[test]
    fn upd_022_a_changed_file_another_key_or_another_namespace_is_refused() {
        let path = file("tamper", b"the release file");
        let sig = sign(7, "ns", b"the release file");
        let changed = file("tamper-changed", b"the release file!");
        assert!(verify_file(&changed, &sig, &public(7), "ns").unwrap_err().contains("does not match the file"));
        assert!(verify_file(&path, &sig, &public(8), "ns").unwrap_err().contains("not signed with the release key"));
        assert!(verify_file(&path, &sign(8, "ns", b"the release file"), &public(7), "ns").unwrap_err().contains("not signed with the release key"));
        assert!(verify_file(&path, &sig, &public(7), "ns2").unwrap_err().contains("another file"));
        assert!(verify_file(&path, &sig, &public(7), "n").unwrap_err().contains("another file"));
        // a signature of other data under this key, put on this file
        assert!(verify_file(&path, &sign(7, "ns", b"other"), &public(7), "ns").unwrap_err().contains("does not match the file"));
        // a signature whose bytes were flipped
        let flipped = sign_edited(7, "ns", b"the release file", |b| *b.last_mut().unwrap() ^= 1);
        assert!(verify_file(&path, &flipped, &public(7), "ns").unwrap_err().contains("does not match the file"));
        assert!(verify_file(&path.with_file_name("missing"), &sig, &public(7), "ns").unwrap_err().contains("could not be read"));
    }

    // spec: UPD-022
    #[test]
    fn upd_022_a_damaged_or_foreign_signature_file_is_refused_in_words() {
        let path = file("damaged", b"x");
        let key = public(7);
        let sig = sign(7, "ns", b"x");
        let check = |text: &str, why: &str| {
            let e = verify_file(&path, text, &key, "ns").unwrap_err();
            assert!(e.contains(why), "{text:?}: {e}");
        };
        check("", "no BEGIN and END");
        check("just text", "no BEGIN and END");
        check(&format!("{ARMOR_BEGIN}\n{ARMOR_END}\n"), "no BEGIN and END");
        check(&sig.replace(ARMOR_END, ""), "no BEGIN and END");
        check(&sig.replacen("U1NIU0lH", "U1NI*0lH", 1), "not base64");
        // cut at every length: always an error, never a panic
        let blob = unarmor(&sig).unwrap();
        for n in 0..blob.len() {
            let cut = sign_edited(7, "ns", b"x", |b| b.truncate(n));
            assert!(verify_file(&path, &cut, &key, "ns").is_err(), "cut at {n}");
        }
        check(&sign_edited(7, "ns", b"x", |b| b[0] = b'X'), "wrong magic");
        check(&sign_edited(7, "ns", b"x", |b| b[9] = 2), "version 2");
        check(&sign_edited(7, "ns", b"x", |b| b.push(0)), "more data");
        // the hash algorithm: sha256 is not accepted
        let sha256 = {
            let mut blob = unarmor(&sig).unwrap();
            let at = blob.windows(6).position(|w| w == b"sha512").unwrap();
            blob[at..at + 6].copy_from_slice(b"sha256");
            blob
        };
        check(&sign_edited(7, "ns", b"x", |b| *b = sha256), "only sha512");
        // the type of the signature
        let wrong_type = {
            let mut blob = unarmor(&sig).unwrap();
            let at = blob.windows(11).rposition(|w| w == KEY_TYPE).unwrap();
            blob[at + 4] = b'x';
            blob
        };
        check(&sign_edited(7, "ns", b"x", |b| *b = wrong_type), "not an ssh-ed25519 signature");
        // trailing data inside the signature blob
        let mut blob = unarmor(&sig).unwrap();
        let at = blob.len() - 64 - 4 - 11 - 4 - 4;
        assert_eq!(&blob[at..at + 4], &[0, 0, 0, 83], "the signature blob");
        blob[at + 3] = 84;
        blob.push(0);
        check(&sign_edited(7, "ns", b"x", |b| *b = blob), "more data");
    }

    // The real tool: a signature made by `ssh-keygen -Y sign` with a throwaway key. Skipped (with
    // a note) where `ssh-keygen` is missing, like the GnuPG tests are.
    // spec: UPD-022
    #[cfg(unix)]
    #[test]
    fn upd_022_a_signature_made_by_ssh_keygen_is_accepted_and_a_changed_file_is_not() {
        use std::process::Command;
        if Command::new("ssh-keygen").arg("-?").output().is_err() {
            eprintln!("skipped: ssh-keygen is not installed");
            return;
        }
        let path = file("ssh-keygen", b"made by the real tool");
        let dir = path.parent().unwrap().to_path_buf();
        let run = |args: &[&str]| {
            let out = Command::new("ssh-keygen").args(args).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "ssh-keygen {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["-q", "-t", "ed25519", "-N", "", "-C", "throwaway", "-f", "key"]);
        let ns = namespace("shale-1.0.0-linux-x86_64.tar.gz");
        run(&["-Y", "sign", "-q", "-f", "key", "-n", &ns, "file"]);
        let key = public_key(&std::fs::read_to_string(dir.join("key.pub")).unwrap()).unwrap();
        let sig = std::fs::read_to_string(dir.join("file.sig")).unwrap();
        verify_file(&path, &sig, &key, &ns).unwrap();
        assert!(verify_file(&path, &sig, &key, "shale-release/other").is_err());
        assert!(verify_file(&path, &sig, &public(7), &ns).is_err(), "not the key of the release");
        std::fs::write(&path, b"made by the real tool, changed").unwrap();
        assert!(verify_file(&path, &sig, &key, &ns).unwrap_err().contains("does not match the file"));
        // and the tool itself agrees with the format: it verifies what Shale accepts
        std::fs::write(&path, b"made by the real tool").unwrap();
        std::fs::write(dir.join("allowed"), format!("shale {}", std::fs::read_to_string(dir.join("key.pub")).unwrap())).unwrap();
        let verified = Command::new("ssh-keygen").args(["-Y", "verify", "-f", "allowed", "-I", "shale", "-n", &ns, "-s", "file.sig"]).current_dir(&dir).stdin(std::fs::File::open(&path).unwrap()).output().unwrap();
        assert!(verified.status.success(), "{}", String::from_utf8_lossy(&verified.stderr));
    }
}
