//! Reading archives for the template sets (TEX-006): zip, tar.gz and folders are unpacked, and
//! nothing that could write outside the folder, link out of it or fill the disk is accepted.
//! The archives are built here, byte by byte, so hostile ones can be made too.

use super::archive::*;
use std::path::{Path, PathBuf};

fn temp(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-archive-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One entry of a test zip, with the fields a hostile archive would lie in.
pub(super) struct Z<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub deflate: bool,
    /// the Unix mode in the high bits of the external attributes (the zip is then "made on Unix")
    mode: Option<u32>,
    flags: u16,
    claimed_size: Option<u32>,
}

pub(super) fn z<'a>(name: &'a str, data: &'a [u8]) -> Z<'a> {
    Z { name, data, deflate: false, mode: None, flags: 0, claimed_size: None }
}

pub(super) fn zip(entries: &[Z]) -> Vec<u8> {
    let (mut body, mut dir) = (Vec::new(), Vec::new());
    for e in entries {
        let packed = if e.deflate { miniz_oxide::deflate::compress_to_vec(e.data, 6) } else { e.data.to_vec() };
        let crc = crc32(e.data);
        let at = body.len() as u32;
        body.extend([0x50, 0x4b, 0x03, 0x04]);
        body.extend([20, 0]);
        body.extend(e.flags.to_le_bytes());
        body.extend((if e.deflate { 8u16 } else { 0 }).to_le_bytes());
        body.extend([0, 0, 0, 0]);
        body.extend(crc.to_le_bytes());
        body.extend((packed.len() as u32).to_le_bytes());
        body.extend((e.data.len() as u32).to_le_bytes());
        body.extend((e.name.len() as u16).to_le_bytes());
        body.extend([0, 0]);
        body.extend(e.name.as_bytes());
        body.extend(&packed);
        dir.extend([0x50, 0x4b, 0x01, 0x02]);
        dir.extend([20u8, if e.mode.is_some() { 3 } else { 0 }]);
        dir.extend([20, 0]);
        dir.extend(e.flags.to_le_bytes());
        dir.extend((if e.deflate { 8u16 } else { 0 }).to_le_bytes());
        dir.extend([0, 0, 0, 0]);
        dir.extend(crc.to_le_bytes());
        dir.extend((packed.len() as u32).to_le_bytes());
        dir.extend(e.claimed_size.unwrap_or(e.data.len() as u32).to_le_bytes());
        dir.extend((e.name.len() as u16).to_le_bytes());
        dir.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        dir.extend((e.mode.unwrap_or(0) << 16).to_le_bytes());
        dir.extend(at.to_le_bytes());
        dir.extend(e.name.as_bytes());
    }
    let (dir_at, dir_len) = (body.len() as u32, dir.len() as u32);
    body.extend(&dir);
    body.extend([0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
    body.extend((entries.len() as u16).to_le_bytes());
    body.extend((entries.len() as u16).to_le_bytes());
    body.extend(dir_len.to_le_bytes());
    body.extend(dir_at.to_le_bytes());
    body.extend([0, 0]);
    body
}

/// A tar header + data.
pub(super) fn tar_entry(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..107].copy_from_slice(b"0000644");
    h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
    h[156] = kind;
    h[257..262].copy_from_slice(b"ustar");
    h[148..156].fill(b' ');
    let sum: u32 = h.iter().map(|&b| b as u32).sum();
    h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    h[155] = b' ';
    let mut out = h;
    out.extend(data);
    out.extend(vec![0u8; (512 - data.len() % 512) % 512]);
    out
}

pub(super) fn tar_gz(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut tar: Vec<u8> = entries.concat();
    tar.extend(vec![0u8; 1024]);
    let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3];
    gz.extend(miniz_oxide::deflate::compress_to_vec(&tar, 6));
    gz.extend(crc32(&tar).to_le_bytes());
    gz.extend((tar.len() as u32).to_le_bytes());
    gz
}

fn listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut es: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        es.sort_by_key(|e| e.file_name());
        for e in es {
            let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if e.path().is_dir() {
                out.push(format!("{rel}/"));
                walk(base, &e.path(), out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out
}

// spec: TEX-006
#[test]
fn the_checksums_are_the_standard_ones() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

// spec: TEX-006
#[test]
fn a_zip_with_stored_and_deflated_files_and_folders_is_unpacked() {
    let d = temp("zip");
    let big = "A line of text that compresses well.\n".repeat(200);
    let bytes = zip(&[z("pkg/", b""), z("pkg/tex/", b""), Z { deflate: true, ..z("pkg/tex/a.cls", big.as_bytes()) }, z("pkg/readme.md", b"hello"), z("pkg/deep/er/b.sty", b"x")]);
    extract(&bytes, &d).unwrap();
    assert_eq!(listing(&d), ["pkg/", "pkg/deep/", "pkg/deep/er/", "pkg/deep/er/b.sty", "pkg/readme.md", "pkg/tex/", "pkg/tex/a.cls"]);
    assert_eq!(std::fs::read_to_string(d.join("pkg/tex/a.cls")).unwrap(), big);
    assert_eq!(single_root(&d), d.join("pkg"), "the one folder everything is in");
    std::fs::remove_dir_all(&d).unwrap();
}

// spec: TEX-006
#[test]
fn a_tar_gz_of_github_with_its_global_header_and_long_names_is_unpacked() {
    let d = temp("targz");
    let long = format!("repo-abc/{}/file.sty", "d".repeat(120));
    // a pax record is "<length> path=<name>\n", the length counting its own digits
    let rest = format!(" path={long}\n");
    let mut len = rest.len() + 1;
    while format!("{len}{rest}").len() != len {
        len += 1;
    }
    let pax_path = format!("{len}{rest}");
    let gz = tar_gz(&[
        tar_entry("pax_global_header", b'g', b"52 comment=e933be93b07008edc9ce77e33b877d15489a476f\n"),
        tar_entry("repo-abc/", b'5', b""),
        tar_entry("repo-abc/tex/", b'5', b""),
        tar_entry("repo-abc/tex/a.cls", b'0', b"class"),
        tar_entry("PaxHeader", b'x', pax_path.as_bytes()),
        tar_entry("cut", b'0', b"long"),
    ]);
    extract(&gz, &d).unwrap();
    let l = listing(&d);
    assert!(l.contains(&"repo-abc/tex/a.cls".to_string()), "{l:?}");
    assert_eq!(std::fs::read(d.join(&long)).unwrap(), b"long", "the pax path replaces the cut name");
    assert!(!l.iter().any(|n| n.contains("pax_global_header")));
    assert_eq!(single_root(&d), d.join("repo-abc"));
    std::fs::remove_dir_all(&d).unwrap();
}

// spec: TEX-006
#[test]
fn entries_that_climb_out_or_start_at_the_root_are_refused_and_nothing_is_written() {
    for name in ["../escape.txt", "a/../../escape.txt", "/etc/escape.txt", "C:/escape.txt", "a\\..\\escape.txt", "x/..", "a/b:stream", "", "tab\there"] {
        let d = temp("slip");
        let r = extract(&zip(&[z("fine.txt", b"ok"), z(name, b"evil")]), &d);
        assert!(r.is_err(), "{name:?} was accepted");
        assert!(listing(&d).is_empty(), "{name:?}: not even the harmless file was written: {:?}", listing(&d));
        assert!(!d.parent().unwrap().join("escape.txt").exists());
        let r = extract(&tar_gz(&[tar_entry("fine.txt", b'0', b"ok"), tar_entry(name, b'0', b"evil")]), &d);
        assert!(r.is_err(), "tar {name:?} was accepted");
        assert!(listing(&d).is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
    assert_eq!(safe_path("a/./b//c").unwrap(), Path::new("a/b/c"), "dots and double slashes are only skipped");
}

// spec: TEX-006
#[test]
fn links_and_special_files_are_refused() {
    let d = temp("links");
    // a zip made on Unix whose entry is a symlink (mode 0120777) or a fifo
    let r = extract(&zip(&[Z { mode: Some(0o120777), ..z("link", b"/etc/passwd") }]), &d);
    assert!(r.unwrap_err().contains("link or a special file"));
    assert!(extract(&zip(&[Z { mode: Some(0o010644), ..z("fifo", b"") }]), &d).is_err());
    // regular files and folders with a mode are fine
    extract(&zip(&[Z { mode: Some(0o100644), ..z("ok", b"x") }, Z { mode: Some(0o040755), ..z("dir/", b"") }]), &d).unwrap();
    // tar: symlink, hard link, device
    for kind in *b"21346" {
        let d2 = temp("tarlink");
        let r = extract(&tar_gz(&[tar_entry("l", kind, b"")]), &d2);
        assert!(r.unwrap_err().contains("link or a special file"), "type {}", kind as char);
        assert!(listing(&d2).is_empty());
    }
    std::fs::remove_dir_all(&d).unwrap();
}

// spec: TEX-006
#[test]
fn doubles_encryption_and_lies_about_sizes_are_refused() {
    let d = temp("lies");
    assert!(extract(&zip(&[z("a", b"1"), z("a", b"2")]), &d).unwrap_err().contains("twice"));
    assert!(extract(&zip(&[z("a", b"1"), z("a/b", b"2")]), &d).unwrap_err().contains("file and as a folder"));
    assert!(extract(&zip(&[Z { flags: 1, ..z("secret", b"x") }]), &d).unwrap_err().contains("encrypted"));
    // a header that claims more than the limit, with a few bytes of data: refused before anything is inflated
    let r = extract(&zip(&[Z { deflate: true, claimed_size: Some((MAX_FILE + 1) as u32), ..z("bomb", b"tiny") }]), &d);
    assert!(r.unwrap_err().contains("larger than"));
    // a claim that is less than the truth, or a changed byte: the checksum or the size says so
    let mut ok = zip(&[z("a", b"hello")]);
    assert!(extract(&ok, &d).is_ok());
    let at = ok.windows(5).position(|w| w == b"hello").unwrap();
    ok[at] = b'j';
    assert!(extract(&ok, &d).unwrap_err().contains("damaged"));
    assert!(extract(&zip(&[Z { deflate: true, claimed_size: Some(3), ..z("short", b"hello world, hello world") }]), &d).is_err());
    // not an archive, and a cut-off one
    assert!(extract(b"just some text", &d).unwrap_err().contains("neither a zip"));
    assert!(extract(&zip(&[z("a", b"x")])[..20], &d).is_err());
    let gz = tar_gz(&[tar_entry("a", b'0', b"x")]);
    assert!(extract(&gz[..gz.len() - 3], &d).is_err());
    let mut bad_tar = tar_gz(&[tar_entry("a", b'0', b"x")]);
    let n = bad_tar.len();
    bad_tar[n - 8] ^= 1;
    assert!(extract(&bad_tar, &d).unwrap_err().contains("checksum"));
    std::fs::remove_dir_all(&d).unwrap();
}

// spec: TEX-006
#[test]
fn too_many_entries_are_refused() {
    let d = temp("many");
    let names: Vec<String> = (0..MAX_ENTRIES + 1).map(|i| format!("f{i}")).collect();
    let entries: Vec<Z> = names.iter().map(|n| z(n, b"")).collect();
    assert!(extract(&zip(&entries), &d).unwrap_err().contains("more than"));
    let tars: Vec<Vec<u8>> = names.iter().map(|n| tar_entry(n, b'0', b"")).collect();
    assert!(extract(&tar_gz(&tars), &d).unwrap_err().contains("more than"));
    std::fs::remove_dir_all(&d).unwrap();
}

// spec: TEX-006
#[test]
fn a_folder_is_copied_and_a_link_in_it_is_refused() {
    let src = temp("copy-src");
    std::fs::create_dir_all(src.join("tex/sub")).unwrap();
    std::fs::write(src.join("tex/a.cls"), "a").unwrap();
    std::fs::write(src.join("tex/sub/b.sty"), "b").unwrap();
    let dest = temp("copy-dest");
    copy_folder(&src, &dest).unwrap();
    assert_eq!(listing(&dest), ["tex/", "tex/a.cls", "tex/sub/", "tex/sub/b.sty"]);
    assert!(copy_folder(&src.join("tex/a.cls"), &dest).unwrap_err().contains("not a folder"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", src.join("tex/evil.sty")).unwrap();
        let dest2 = temp("copy-dest2");
        assert!(copy_folder(&src, &dest2).unwrap_err().contains("link or a special file"));
        std::fs::remove_dir_all(&dest2).unwrap();
    }
    std::fs::remove_dir_all(&src).unwrap();
    std::fs::remove_dir_all(&dest).unwrap();
}

// spec: TEX-006
#[test]
fn the_root_folder_of_an_archive_is_stepped_into_only_when_it_is_the_only_thing() {
    let d = temp("root");
    std::fs::create_dir_all(d.join("only")).unwrap();
    assert_eq!(single_root(&d), d.join("only"));
    std::fs::write(d.join("file.txt"), "x").unwrap();
    assert_eq!(single_root(&d), d, "a folder and a file: no root folder");
    let e = temp("root-tex");
    std::fs::create_dir_all(e.join("tex")).unwrap();
    assert_eq!(single_root(&e), e, "a folder named tex is the set's own folder, not a wrapper");
    std::fs::remove_dir_all(&d).unwrap();
    std::fs::remove_dir_all(&e).unwrap();
}
