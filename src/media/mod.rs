//! Inserting files with `[!`: path completion (zsh-style) and copying the chosen file into
//! `<vault>/z_media/<year>/<month>/<YYMMDD>_<file name>`.
//!
//! `[!/…`  starts at the vault root, `[!~/…` at the home folder, `[!…` (anything else) at the
//! vault root as well.

pub mod cover;
pub mod decode;
pub mod download;
pub mod engine;
pub mod output;
pub mod player;
#[cfg(test)]
pub mod testdata;
#[cfg(test)]
mod ui_tests;
pub mod widget;

pub use player::MediaPlayer;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const MEDIA_DIR: &str = "z_media";
const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

/// The sound files that get a player when they are embedded (MEDIA-001).
const AUDIO_EXTS: [&str; 8] = ["mp3", "ogg", "oga", "opus", "flac", "m4a", "aac", "wav"];

pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// A picture from disk as a texture, loaded once per file version and kept in egui's memory.
/// Large pictures are scaled down to at most 2048 px. None if it can't be read.
pub fn texture(ctx: &eframe::egui::Context, abs: &Path) -> Option<eframe::egui::TextureHandle> {
    use eframe::egui;
    let mtime = fs::metadata(abs).and_then(|m| m.modified()).ok();
    let id = egui::Id::unique(("shale-image", abs.to_path_buf(), mtime));
    if let Some(cached) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return cached;
    }
    let loaded = image::open(abs).ok().map(|img| {
        let img = if img.width() > 2048 || img.height() > 2048 { img.thumbnail(2048, 2048) } else { img };
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        ctx.load_texture(abs.to_string_lossy(), color, egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp(id, loaded.clone()));
    loaded
}

/// An image widget for a picture file, fitted into max_w × max_h.
pub fn image_widget(ctx: &eframe::egui::Context, abs: &Path, max_w: f32, max_h: f32) -> Option<eframe::egui::Image<'static>> {
    let tex = texture(ctx, abs)?;
    Some(eframe::egui::Image::from_texture(eframe::egui::load::SizedTexture::from_handle(&tex)).max_width(max_w).max_height(max_h))
}

/// If the cursor sits after an unfinished `[!path`, returns (byte where `[!` starts, typed path).
pub fn media_query(text: &str, cursor_byte: usize) -> Option<(usize, String)> {
    let line_start = text[..cursor_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let before = &text[line_start..cursor_byte];
    let open = before.rfind("[!")?;
    // `![` image syntax is not a trigger, only `[!`
    let typed = &before[open + 2..];
    if typed.contains(']') || typed.contains('[') || typed.chars().count() > 300 {
        return None;
    }
    Some((line_start + open, typed.to_string()))
}

/// Where a typed path points: (folder to list, partial name being typed, the typed folder part).
pub fn split_typed(typed: &str, vault_root: &Path, home: &Path) -> (PathBuf, String, String) {
    let (base, rest) = if let Some(r) = typed.strip_prefix("~/") {
        (home.to_path_buf(), r)
    } else if typed == "~" {
        (home.to_path_buf(), "")
    } else if let Some(r) = typed.strip_prefix('/') {
        (vault_root.to_path_buf(), r)
    } else {
        (vault_root.to_path_buf(), typed)
    };
    let (dir_part, partial) = match rest.rfind('/') {
        Some(p) => (&rest[..p + 1], &rest[p + 1..]),
        None => ("", rest),
    };
    let dir = if dir_part.is_empty() { base } else { base.join(dir_part.trim_end_matches('/')) };
    let typed_dir = &typed[..typed.len() - partial.len()];
    (dir, partial.to_string(), typed_dir.to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

/// Entries of `dir` starting with `partial` (case-insensitive). Hidden entries only when the
/// partial name starts with a dot. Folders first.
pub fn list_matches(dir: &Path, partial: &str) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let lp = partial.to_lowercase();
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !partial.starts_with('.') {
                return None;
            }
            if !name.to_lowercase().starts_with(&lp) {
                return None;
            }
            let is_dir = e.path().is_dir();
            Some(Entry { name, is_dir })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// Longest common prefix of the names (case-insensitive compare, spelling of the first name).
pub fn common_prefix(entries: &[Entry]) -> String {
    let Some(first) = entries.first() else { return String::new() };
    let mut len = first.name.len();
    for e in &entries[1..] {
        let mut l = 0;
        for ((ia, ca), cb) in first.name.char_indices().zip(e.name.chars()) {
            if ca.to_lowercase().eq(cb.to_lowercase()) {
                l = ia + ca.len_utf8();
            } else {
                break;
            }
        }
        len = len.min(l);
    }
    first.name[..len].to_string()
}

/// What Tab does: returns the new typed path, or None if nothing can be completed.
pub fn tab_complete(typed: &str, vault_root: &Path, home: &Path) -> Option<String> {
    let (dir, partial, typed_dir) = split_typed(typed, vault_root, home);
    let m = list_matches(&dir, &partial);
    match m.len() {
        0 => None,
        1 => {
            let e = &m[0];
            let done = format!("{typed_dir}{}{}", e.name, if e.is_dir { "/" } else { "" });
            (done != typed).then_some(done)
        }
        _ => {
            let cp = common_prefix(&m);
            (cp.chars().count() > partial.chars().count()).then(|| format!("{typed_dir}{cp}"))
        }
    }
}

/// Today's local date (year, month, day), in the computer's time zone (`time::local_offset`).
pub fn local_date() -> (i32, u32, u32) {
    crate::calendar::time::civil_from_days(crate::calendar::time::today())
}

/// `z_media/2026/11/261112_photo.png` for a file called `photo.png` on 2026-11-12.
pub fn media_rel_path(file_name: &str, date: (i32, u32, u32)) -> PathBuf {
    let (y, m, d) = date;
    PathBuf::from(MEDIA_DIR).join(format!("{y:04}")).join(format!("{m:02}")).join(format!("{:02}{m:02}{d:02}_{file_name}", y.rem_euclid(100)))
}

/// Bring a file into the vault. Files already inside the vault are linked where they are;
/// files from elsewhere are copied to the media folder (a number is added if the name is taken).
/// Returns the vault-relative path to link to.
pub fn import_file(vault_root: &Path, src: &Path, date: (i32, u32, u32)) -> io::Result<PathBuf> {
    let root = vault_root.canonicalize().unwrap_or_else(|_| vault_root.to_path_buf());
    let src_c = src.canonicalize()?;
    if let Ok(rel) = src_c.strip_prefix(&root) {
        return Ok(rel.to_path_buf());
    }
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| io::Error::other("not a file"))?;
    // a name taken in the meantime (AlreadyExists) is never overwritten: pick the next one
    loop {
        let rel = free_media_path(vault_root, &name, date)?;
        match crate::fsutil::copy_new(src, &vault_root.join(&rel)) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            r => return r.map(|_| rel),
        }
    }
}

/// A media path for `name` that isn't taken yet (a number is added if needed); makes the folder.
fn free_media_path(vault_root: &Path, name: &str, date: (i32, u32, u32)) -> io::Result<PathBuf> {
    // names have to work inside [[links]]
    let safe = crate::markdown::link_safe_name(name);
    let name = if safe.trim().is_empty() { "file" } else { safe.as_str() };
    let mut rel = media_rel_path(name, date);
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = Path::new(name).extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let mut i = 1;
    while vault_root.join(&rel).exists() {
        rel = media_rel_path(&format!("{stem} {i}{ext}"), date);
        i += 1;
    }
    if let Some(p) = vault_root.join(&rel).parent() {
        fs::create_dir_all(p)?;
    }
    Ok(rel)
}

/// Store data (an e-mail attachment, a message) in the media folder under `name`.
pub fn save_bytes(vault_root: &Path, name: &str, data: &[u8], date: (i32, u32, u32)) -> io::Result<PathBuf> {
    let clean: String = name.chars().map(|c| if matches!(c, '/' | '\\' | '\0') { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.');
    loop {
        let rel = free_media_path(vault_root, if clean.is_empty() { "file" } else { clean }, date)?;
        match crate::fsutil::create_new(&vault_root.join(&rel), data) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            r => return r.map(|_| rel),
        }
    }
}

/// The markdown to insert: images are embedded `![[…]]`, other files linked `[[…]]`.
pub fn link_for(rel: &Path) -> String {
    let s = crate::vault::rel_to_string(rel);
    if is_image(rel) {
        format!("![[{s}]]")
    } else {
        format!("[[{s}]]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: VAULT-012
    #[test]
    fn saved_names_work_in_links() {
        let root = std::env::temp_dir().join(format!("shale-media-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let rel = save_bytes(&root, "Your receipt from Shop, Limited #2583-3353.eml", b"x", (2026, 11, 12)).unwrap();
        assert_eq!(crate::vault::rel_to_string(&rel), "z_media/2026/11/261112_Your receipt from Shop, Limited 2583-3353.eml");
        let mut v = crate::vault::Vault::open(&root).unwrap();
        v.rescan();
        let link = link_for(&rel);
        let l = &crate::markdown::find_links(&link)[0];
        assert_eq!(v.resolve(&l.target, None), Some(rel.clone()), "the link is blue");
        // an older file that still has the # in its name is found as well
        fs::write(root.join("z_media/2026/11/261112_Old #1.eml"), b"x").unwrap();
        v.rescan();
        let l = &crate::markdown::find_links("[[z_media/2026/11/261112_Old #1.eml]]")[0];
        assert!(v.resolve(&l.target, None).is_some());
        let _ = fs::remove_dir_all(&root);
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-media-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: EDIT-002
    #[test]
    fn query_detection() {
        assert_eq!(media_query("x [!~/Pic", 9), Some((2, "~/Pic".into())));
        assert_eq!(media_query("[!/", 3), Some((0, "/".into())));
        assert_eq!(media_query("[!", 2), Some((0, "".into())));
        assert_eq!(media_query("![[a]] [x]", 10), None);
        assert_eq!(media_query("[!/a] after", 11), None);
    }

    // spec: EDIT-002
    #[test]
    fn splitting_paths() {
        let v = Path::new("/vault");
        let h = Path::new("/home/me");
        assert_eq!(split_typed("~/Pic", v, h), (PathBuf::from("/home/me"), "Pic".into(), "~/".into()));
        assert_eq!(split_typed("~/Pictures/ca", v, h), (PathBuf::from("/home/me/Pictures"), "ca".into(), "~/Pictures/".into()));
        assert_eq!(split_typed("/", v, h), (PathBuf::from("/vault"), "".into(), "/".into()));
        assert_eq!(split_typed("/Projects/G", v, h), (PathBuf::from("/vault/Projects"), "G".into(), "/Projects/".into()));
    }

    // spec: EDIT-002
    #[test]
    fn tab_completion_like_a_shell() {
        let home = tmp("home");
        fs::create_dir_all(home.join("Pictures/holiday")).unwrap();
        fs::create_dir_all(home.join("Projects")).unwrap();
        fs::write(home.join("Pictures/cat.png"), b"x").unwrap();
        fs::write(home.join("Pictures/castle.jpg"), b"x").unwrap();
        let vault = tmp("vault");
        // "~/P" matches Pictures and Projects: nothing common beyond "P"
        assert_eq!(tab_complete("~/P", &vault, &home), None);
        assert_eq!(tab_complete("~/Pi", &vault, &home), Some("~/Pictures/".into()));
        assert_eq!(tab_complete("~/Pictures/c", &vault, &home), Some("~/Pictures/ca".into()));
        assert_eq!(tab_complete("~/Pictures/cat", &vault, &home), Some("~/Pictures/cat.png".into()));
        assert_eq!(tab_complete("~/Pictures/h", &vault, &home), Some("~/Pictures/holiday/".into()));
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&vault);
    }

    // spec: EDIT-003
    #[test]
    fn importing_copies_into_dated_media_folder() {
        let home = tmp("src");
        let vault = tmp("dst");
        fs::write(home.join("cat.png"), b"meow").unwrap();
        let date = (2026, 11, 12);
        assert_eq!(media_rel_path("cat.png", date), PathBuf::from("z_media/2026/11/261112_cat.png"));
        let rel = import_file(&vault, &home.join("cat.png"), date).unwrap();
        assert_eq!(rel, PathBuf::from("z_media/2026/11/261112_cat.png"));
        assert_eq!(fs::read(vault.join(&rel)).unwrap(), b"meow");
        // same name again gets a number
        let rel2 = import_file(&vault, &home.join("cat.png"), date).unwrap();
        assert_eq!(rel2, PathBuf::from("z_media/2026/11/261112_cat 1.png"));
        // files already in the vault are linked in place
        fs::write(vault.join("in.pdf"), b"pdf").unwrap();
        assert_eq!(import_file(&vault, &vault.join("in.pdf"), date).unwrap(), PathBuf::from("in.pdf"));
        assert_eq!(link_for(&rel), "![[z_media/2026/11/261112_cat.png]]");
        assert_eq!(link_for(Path::new("in.pdf")), "[[in.pdf]]");
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&vault);
    }

    // spec: EDIT-003
    #[test]
    fn local_date_is_sane() {
        let (y, m, d) = local_date();
        assert!(y >= 2024 && (1..=12).contains(&m) && (1..=31).contains(&d));
    }
}
