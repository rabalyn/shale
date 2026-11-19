//! Reading a template archive into a folder (TEX-006): a `.zip` (CTAN, GitHub's "Download ZIP")
//! or a `.tar.gz` (GitHub's archive of a commit), and copying a local folder. The system `tar`
//! cannot read a zip on Linux and the programs differ in what they do with a hostile name, so one
//! small reader of our own does both, on `miniz_oxide`, and one function decides which names and
//! kinds of entry are accepted: nothing is written outside the folder (no `..`, no absolute
//! name, no drive or stream name, no backslash), a link or a device is refused, a name that comes
//! twice is refused, and the sizes are limited so that a small archive cannot fill the disk.
//! Everything is read and checked in memory before the first file is written.

use std::path::{Component, Path, PathBuf};

/// The most bytes an archive may unpack to, in all and per file.
pub const MAX_TOTAL: u64 = 256 << 20;
pub const MAX_FILE: u64 = 64 << 20;
/// What an archive may unpack to: the most bytes in all and per file.
#[derive(Clone, Copy)]
pub struct Limits {
    pub total: u64,
    pub file: u64,
}

impl Limits {
    /// The limits of a template archive.
    pub const TEMPLATES: Limits = Limits { total: MAX_TOTAL, file: MAX_FILE };
}

/// The most entries (files and folders) of an archive or a folder.
pub const MAX_ENTRIES: usize = 20_000;

/// One thing of an archive: a folder (`data` none) or a file.
struct Entry {
    path: PathBuf,
    data: Option<Vec<u8>>,
}

/// A name of an archive as a relative path of plain components, or why it is refused.
pub(super) fn safe_path(raw: &str) -> Result<PathBuf, String> {
    let bad = |why: &str| Err(format!("the archive has an entry named {raw:?} ({why}): refused"));
    if raw.is_empty() {
        return bad("empty");
    }
    if raw.contains('\\') || raw.contains('\0') || raw.chars().any(char::is_control) {
        return bad("a backslash or a control character");
    }
    if raw.starts_with('/') {
        return bad("an absolute path");
    }
    let mut out = PathBuf::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => return bad("it climbs out of the folder"),
            p if p.contains(':') => return bad("a drive or stream name"),
            p => out.push(p),
        }
    }
    if out.as_os_str().is_empty() {
        return bad("no name");
    }
    // belt and braces: whatever the splitting did, the result has only plain components
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return bad("not a plain relative path");
    }
    Ok(out)
}

/// CRC-32 (the checksum of zip and gzip).
pub(super) fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The SHA-256 of bytes as lower case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as u64)
}

fn inflate(raw: &[u8], size: u64, what: &str) -> Result<Vec<u8>, String> {
    miniz_oxide::inflate::decompress_to_vec_with_limit(raw, size as usize).map_err(|e| format!("{what} cannot be unpacked ({e:?})"))
}

// ---------------------------------------------------------------- zip

/// The entries of a zip: central directory first, then each file's data (stored or deflated).
fn read_zip(b: &[u8], lim: Limits) -> Result<Vec<Entry>, String> {
    let broken = || "the zip file is damaged".to_string();
    let from = b.len().saturating_sub(22 + 65535);
    let eocd = (from..=b.len().saturating_sub(22)).rev().find(|&i| b.get(i..i + 4) == Some(&[0x50, 0x4b, 0x05, 0x06])).ok_or_else(broken)?;
    let count = u16_at(b, eocd + 10).ok_or_else(broken)?;
    let dir_at = u32_at(b, eocd + 16).ok_or_else(broken)? as usize;
    if count == 0xFFFF || dir_at == 0xFFFF_FFFF {
        return Err("zip64 files are not supported".into());
    }
    if count > MAX_ENTRIES {
        return Err(format!("the archive has more than {MAX_ENTRIES} entries: refused"));
    }
    let (mut at, mut total, mut entries) = (dir_at, 0u64, Vec::new());
    for _ in 0..count {
        if b.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(broken());
        }
        let (made_by, flags, method) = (u16_at(b, at + 4).ok_or_else(broken)?, u16_at(b, at + 8).ok_or_else(broken)?, u16_at(b, at + 10).ok_or_else(broken)?);
        let (crc, csize, usize_) = (u32_at(b, at + 16).ok_or_else(broken)?, u32_at(b, at + 20).ok_or_else(broken)?, u32_at(b, at + 24).ok_or_else(broken)?);
        let (n, m, k) = (u16_at(b, at + 28).ok_or_else(broken)?, u16_at(b, at + 30).ok_or_else(broken)?, u16_at(b, at + 32).ok_or_else(broken)?);
        let attrs = u32_at(b, at + 38).ok_or_else(broken)?;
        let local = u32_at(b, at + 42).ok_or_else(broken)? as usize;
        let raw_name = std::str::from_utf8(b.get(at + 46..at + 46 + n).ok_or_else(broken)?).map_err(|_| "the archive has a name that is not UTF-8: refused".to_string())?;
        at += 46 + n + m + k;
        // a link or another special file: the Unix mode is in the high bits when the zip was made on Unix
        let mode = (attrs >> 16) as u32;
        if made_by >> 8 == 3 && mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 && mode & 0o170000 != 0o040000 {
            return Err(format!("the archive has {raw_name:?}, a link or a special file: refused"));
        }
        let path = safe_path(raw_name)?;
        if raw_name.ends_with('/') {
            entries.push(Entry { path, data: None });
            continue;
        }
        if flags & 1 != 0 {
            return Err(format!("{raw_name:?} is encrypted: refused"));
        }
        if usize_ > lim.file {
            return Err(format!("{raw_name:?} is larger than {} MB: refused", lim.file >> 20));
        }
        total += usize_;
        if total > lim.total {
            return Err(format!("the archive unpacks to more than {} MB: refused", lim.total >> 20));
        }
        if b.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
            return Err(broken());
        }
        let start = local + 30 + u16_at(b, local + 26).ok_or_else(broken)? + u16_at(b, local + 28).ok_or_else(broken)?;
        let raw = b.get(start..start + csize as usize).ok_or_else(broken)?;
        let data = match method {
            0 => raw.to_vec(),
            8 => inflate(raw, usize_, raw_name)?,
            other => return Err(format!("{raw_name:?} uses the compression method {other}, which is not supported")),
        };
        if data.len() as u64 != usize_ || crc32(&data) as u64 != crc {
            return Err(format!("{raw_name:?} is damaged (its checksum is wrong)"));
        }
        entries.push(Entry { path, data: Some(data) });
    }
    Ok(entries)
}

// ---------------------------------------------------------------- tar.gz

/// The deflated part of a gzip file (one member), checked against its trailer.
fn gunzip(b: &[u8], lim: Limits) -> Result<Vec<u8>, String> {
    let broken = || "the gzip file is damaged".to_string();
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b || b[2] != 8 {
        return Err(broken());
    }
    let flags = b[3];
    let mut at = 10;
    if flags & 4 != 0 {
        at += 2 + u16_at(b, at).ok_or_else(broken)?;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            at += b.get(at..).ok_or_else(broken)?.iter().position(|&c| c == 0).ok_or_else(broken)? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let body = b.get(at..b.len() - 8).ok_or_else(broken)?;
    let data = inflate(body, lim.total, "the gzip file")?;
    let (crc, size) = (u32_at(b, b.len() - 8).ok_or_else(broken)?, u32_at(b, b.len() - 4).ok_or_else(broken)?);
    if crc32(&data) as u64 != crc || data.len() as u64 & 0xFFFF_FFFF != size {
        return Err("the gzip file is damaged (its checksum is wrong)".into());
    }
    Ok(data)
}

/// An octal field of a tar header.
fn octal(field: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(field).ok()?.trim_matches(|c: char| c == '\0' || c == ' ');
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

/// A NUL-ended text field.
fn text(field: &[u8]) -> String {
    String::from_utf8_lossy(field.split(|&c| c == 0).next().unwrap_or(&[])).into_owned()
}

/// The `path` of a pax extended header (`<length> path=<name>\n` records).
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest.iter().position(|&c| c == b' ')?;
        let len: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        let record = rest.get(space + 1..len)?;
        if let Some(p) = record.strip_prefix(b"path=") {
            return Some(String::from_utf8_lossy(p.strip_suffix(b"\n").unwrap_or(p)).into_owned());
        }
        rest = rest.get(len..)?;
    }
    None
}

/// The entries of an (uncompressed) tar.
fn read_tar(b: &[u8], lim: Limits) -> Result<Vec<Entry>, String> {
    let broken = || "the tar file is damaged".to_string();
    let (mut at, mut total, mut entries) = (0usize, 0u64, Vec::new());
    let mut long_name: Option<String> = None;
    while at + 512 <= b.len() {
        let h = &b[at..at + 512];
        if h.iter().all(|&c| c == 0) {
            break;
        }
        let sum: u64 = h.iter().enumerate().map(|(i, &c)| if (148..156).contains(&i) { 32 } else { c as u64 }).sum();
        if octal(&h[148..156]) != Some(sum) {
            return Err(broken());
        }
        let size = octal(&h[124..136]).ok_or_else(broken)?;
        let kind = h[156];
        let body_at = at + 512;
        let end = body_at.checked_add(size as usize).filter(|&e| e <= b.len()).ok_or_else(broken)?;
        let body = &b[body_at..end];
        at = body_at + (size as usize).div_ceil(512) * 512;
        let mut name = text(&h[0..100]);
        if &h[257..262] == b"ustar" && h[345] != 0 {
            name = format!("{}/{name}", text(&h[345..500]));
        }
        match kind {
            // global header of git archives (the commit id): nothing to unpack
            b'g' => continue,
            b'x' => {
                long_name = pax_path(body);
                continue;
            }
            b'L' => {
                long_name = Some(text(body));
                continue;
            }
            _ => {}
        }
        let name = long_name.take().unwrap_or(name);
        match kind {
            b'5' => entries.push(Entry { path: safe_path(&name)?, data: None }),
            b'0' | 0 | b'7' => {
                if size > lim.file {
                    return Err(format!("{name:?} is larger than {} MB: refused", lim.file >> 20));
                }
                total += size;
                if total > lim.total {
                    return Err(format!("the archive unpacks to more than {} MB: refused", lim.total >> 20));
                }
                entries.push(Entry { path: safe_path(&name)?, data: Some(body.to_vec()) });
            }
            _ => return Err(format!("the archive has {name:?}, a link or a special file: refused")),
        }
        if entries.len() > MAX_ENTRIES {
            return Err(format!("the archive has more than {MAX_ENTRIES} entries: refused"));
        }
    }
    Ok(entries)
}

// ---------------------------------------------------------------- writing

/// Unpack `bytes` (a zip or a gzipped tar, told apart by their first bytes) into `dest`, a folder
/// that exists and is empty. Nothing is written when any entry is refused.
pub fn extract(bytes: &[u8], dest: &Path) -> Result<(), String> {
    extract_with(bytes, dest, Limits::TEMPLATES)
}

/// [`extract`] with other size limits (the program of an update is larger than a template).
pub fn extract_with(bytes: &[u8], dest: &Path, lim: Limits) -> Result<(), String> {
    let entries = if bytes.starts_with(b"PK") {
        read_zip(bytes, lim)?
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        read_tar(&gunzip(bytes, lim)?, lim)?
    } else {
        return Err("that is neither a zip file nor a .tar.gz file".into());
    };
    let mut seen = std::collections::HashSet::new();
    for e in &entries {
        if e.data.is_some() && !seen.insert(e.path.clone()) {
            return Err(format!("the archive has {} twice: refused", e.path.display()));
        }
    }
    // no file may be a folder of another
    for e in entries.iter().filter(|e| e.data.is_some()) {
        if e.path.ancestors().skip(1).any(|a| seen.contains(a)) {
            return Err(format!("the archive has {} as a file and as a folder: refused", e.path.display()));
        }
    }
    for e in entries {
        let to = dest.join(&e.path);
        match e.data {
            None => std::fs::create_dir_all(&to).map_err(|x| format!("{} could not be made ({x})", to.display()))?,
            Some(data) => {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|x| format!("{} could not be made ({x})", parent.display()))?;
                }
                crate::fsutil::create_new(&to, &data).map_err(|x| format!("{} could not be written ({x})", to.display()))?;
            }
        }
    }
    Ok(())
}

/// Copy the folder `src` into the empty folder `dest`. A link (which could lead anywhere) and
/// anything that is not a plain file or folder is refused, and so is a folder that is too big.
pub fn copy_folder(src: &Path, dest: &Path) -> Result<(), String> {
    fn walk(from: &Path, to: &Path, count: &mut usize, total: &mut u64) -> Result<(), String> {
        for entry in std::fs::read_dir(from).map_err(|e| format!("{} cannot be read ({e})", from.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let (path, target) = (entry.path(), to.join(entry.file_name()));
            let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{} cannot be read ({e})", path.display()))?;
            *count += 1;
            if *count > MAX_ENTRIES {
                return Err(format!("the folder has more than {MAX_ENTRIES} files: refused"));
            }
            if meta.is_dir() {
                std::fs::create_dir(&target).map_err(|e| format!("{} could not be made ({e})", target.display()))?;
                walk(&path, &target, count, total)?;
            } else if meta.is_file() {
                *total += meta.len();
                if meta.len() > MAX_FILE || *total > MAX_TOTAL {
                    return Err(format!("the folder is larger than {} MB: refused", MAX_TOTAL >> 20));
                }
                crate::fsutil::copy_new(&path, &target).map_err(|e| format!("{} could not be copied ({e})", path.display()))?;
            } else {
                return Err(format!("{} is a link or a special file: refused", path.display()));
            }
        }
        Ok(())
    }
    let meta = std::fs::symlink_metadata(src).map_err(|e| format!("{} cannot be read ({e})", src.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", src.display()));
    }
    walk(src, dest, &mut 0, &mut 0)
}

/// The folder an unpacked archive keeps everything in (GitHub's `repo-<commit>/`, CTAN's
/// `tuda-ci/`): `dir` itself when there is more than one thing in it.
pub fn single_root(dir: &Path) -> PathBuf {
    let mut it = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return dir.to_path_buf(),
    };
    match (it.next(), it.next()) {
        (Some(Ok(only)), None) if only.file_type().is_ok_and(|t| t.is_dir()) && !matches!(only.file_name().to_str(), Some("tex" | "kinds" | "logo")) => only.path(),
        _ => dir.to_path_buf(),
    }
}
