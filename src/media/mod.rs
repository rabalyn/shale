//! Inserting files with `[!`: path completion (zsh-style) and copying the chosen file into
//! `<vault>/z_media/<year>/<month>/<YYMMDD>_<file name>`.
//!
//! `[!/…`  starts at the vault root, `[!~/…` at the home folder, `[!…` (anything else) at the
//! vault root as well.

pub mod copy_in;
pub mod cover;
pub mod decode;
pub mod download;
pub mod embed;
pub mod engine;
pub mod ffmpeg;
pub mod intake;
pub mod linked;
pub mod look;
pub mod output;
pub mod outside;
pub mod player;
#[cfg(test)]
pub mod testdata;
#[cfg(test)]
mod ui_tests;
pub mod video;
mod wavfix;
pub mod widget;

pub use player::MediaPlayer;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const MEDIA_DIR: &str = "z_media";
const IMAGE_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];
/// The one list of media file extensions (lower case): sounds. Import (IMP-031), the player
/// (MEDIA-001) and the Git LFS patterns (SYNC-017) all come from `AUDIO_EXTS` and `VIDEO_EXTS` (#206).
pub const AUDIO_EXTS: [&str; 8] = ["mp3", "ogg", "oga", "opus", "m4a", "aac", "wav", "flac"];
/// The video half of the one list; every one of them has a player (MEDIA-009), decoded by ffmpeg.
pub const VIDEO_EXTS: [&str; 7] = ["mp4", "m4v", "webm", "mkv", "mov", "avi", "ogv"];

/// Every audio and video extension, lower case, in the order of the two lists.
pub fn media_extensions() -> impl Iterator<Item = &'static str> {
    AUDIO_EXTS.into_iter().chain(VIDEO_EXTS)
}

/// What a media file is (imports and pastes bring these in as files, IMP-031, EDIT-019).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Pdf,
    Audio,
    Video,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Image => "Image",
            Kind::Pdf => "PDF",
            Kind::Audio => "Audio",
            Kind::Video => "Video",
        }
    }
}

/// The media kind of a file name (by its extension, any case), None for anything else.
pub fn media_kind(p: &Path) -> Option<Kind> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some(Kind::Image)
    } else if ext == "pdf" {
        Some(Kind::Pdf)
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        Some(Kind::Audio)
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        Some(Kind::Video)
    } else {
        None
    }
}

/// The file extension for a content type of a media file ("image/jpeg" → "jpg"), None if it is none.
#[cfg(target_os = "linux")]
pub fn ext_for_mime(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "application/pdf" => "pdf",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/aac" => "aac",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/flac" | "audio/x-flac" => "flac",
        "video/mp4" => "mp4",
        "video/x-m4v" => "m4v",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "video/x-msvideo" => "avi",
        "video/ogg" => "ogv",
        _ => return None,
    })
}

/// `%XX` decoded (a `%` not followed by two hex digits stays as it is).
pub fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = |c: u8| (c as char).to_digit(16);
            if let (Some(x), Some(y)) = (h(b[i + 1]), h(b[i + 2])) {
                bytes.push((x * 16 + y) as u8);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The path of a `file://` address (`file://localhost/…` too, and `file:///C:/…` on Windows), `%XX` decoded; None for any other address.
pub fn file_uri_path(uri: &str) -> Option<PathBuf> {
    file_uri_path_for(uri, cfg!(windows))
}

/// [`file_uri_path`] for Windows (`windows`) or for any other system, so that both forms can be tested everywhere.
pub fn file_uri_path_for(uri: &str, windows: bool) -> Option<PathBuf> {
    let rest = uri.trim().strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let path = percent_decode(rest);
    // Windows writes `file:///C:/Users/x`: the slash before the drive letter is not part of the path
    let drive = path.as_bytes().get(1..3).is_some_and(|d| d[0].is_ascii_alphabetic() && d[1] == b':');
    let path = if windows && drive { &path[1..] } else { &path };
    // on Windows the path has the separators of Windows only (`C:\Users\x`, `\\host\share`), so it
    // is shown, compared and joined like any other path of the system
    Some(PathBuf::from(if windows { path.replace('/', "\\") } else { path.to_string() }))
}

/// The media files of pasted text: every non-empty line is a `file://` address or an absolute
/// path of an existing image, PDF, audio or video file (what a file manager copies). None for any other text.
pub fn pasted_files(text: &str) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let p = if l.starts_with("file://") { file_uri_path(l)? } else { PathBuf::from(l) };
        if !p.is_absolute() || media_kind(&p).is_none() || !p.is_file() {
            return None;
        }
        out.push(p);
    }
    (!out.is_empty()).then_some(out)
}

/// A picture as PNG bytes (RGBA, from the clipboard).
fn png_bytes(img: &eframe::egui::ColorImage) -> Result<Vec<u8>, String> {
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let buf = image::RgbaImage::from_raw(img.width() as u32, img.height() as u32, rgba).ok_or("the picture has a wrong size")?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// What pasting text or a picture into a note does (EDIT-019): saves the media into the vault, or
/// links it where it is (`how`, MEDIA-019, for a file from outside the vault).
/// `Some((text to insert, message))` if the paste was media (the message names what was saved or
/// linked, or what failed); None for a paste that is just text.
pub fn paste_text_to_links(vault_root: &Path, libraries: &[outside::Library], text: &str, how: intake::Choice, date: (i32, u32, u32)) -> Option<(String, String)> {
    let files = pasted_files(text)?;
    let mut links = Vec::new();
    let mut saved = 0;
    let mut linked = 0;
    let mut failed = Vec::new();
    for f in &files {
        match intake::bring(vault_root, libraries, f, how, date) {
            Ok(taken) => {
                links.push(taken.link());
                if matches!(taken, intake::Taken::Linked { .. }) {
                    linked += 1;
                } else {
                    saved += 1;
                }
            }
            Err(e) => failed.push(format!("“{}”: {e}", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    let mut parts = Vec::new();
    match saved {
        0 => {}
        1 => parts.push("Saved 1 file to the media folder".to_string()),
        n => parts.push(format!("Saved {n} files to the media folder")),
    }
    match linked {
        0 => {}
        1 => parts.push("Linked 1 file where it is".to_string()),
        n => parts.push(format!("Linked {n} files where they are")),
    }
    if !failed.is_empty() {
        parts.push(format!("could not copy {}", failed.join(", ")));
    }
    Some((links.join("\n"), parts.join("; ")))
}

/// True if pasted text is nothing but one `<img …>` tag: what a browser puts on the clipboard next
/// to the picture when an image is copied (egui pastes that text and never offers the picture).
pub fn is_lone_img_tag(text: &str) -> bool {
    let mut t = text.trim();
    // Firefox puts a `<meta http-equiv=…>` tag in front of the fragment
    while t.len() > 5 && t[..5].eq_ignore_ascii_case("<meta") {
        match t.find('>') {
            Some(end) => t = t[end + 1..].trim_start(),
            None => return false,
        }
    }
    t.len() > 5 && t[..4].eq_ignore_ascii_case("<img") && t[4..5].trim().is_empty() && t.ends_with('>') && t.matches('<').count() == 1
}

/// The picture on the system clipboard, if there is one.
pub fn clipboard_image() -> Option<eframe::egui::ColorImage> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    Some(eframe::egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.bytes))
}

/// Saves a pasted picture as PNG in the media folder; returns the link to insert.
pub fn paste_image_to_link(vault_root: &Path, img: &eframe::egui::ColorImage, date: (i32, u32, u32)) -> Result<String, String> {
    let png = png_bytes(img)?;
    let rel = save_bytes(vault_root, "Pasted image.png", &png, date).map_err(|e| format!("could not save the picture: {e}"))?;
    Ok(link_for(&rel))
}

/// A sound file, which gets a player when it is embedded (MEDIA-001); the list is the import list.
pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// The video files that get a player when they are embedded (MEDIA-001, MEDIA-009): all of
/// `VIDEO_EXTS`, the import list.
pub fn is_video(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// A sound or a video: what the media player shows.
pub fn is_media(p: &Path) -> bool {
    is_audio(p) || is_video(p)
}

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// A picture file for a frame, at once (MEDIA-024): `Loading` while the file is looked up and
/// decoded on a worker, then its texture, loaded once per file version. Large pictures are scaled
/// down to at most 2048 px. `Broken` if it is not there or can't be read.
pub fn picture(ctx: &eframe::egui::Context, abs: &Path) -> look::Pic {
    look::Looks::of(ctx).picture(ctx, abs)
}

/// The texture of a picture file once it is loaded; None while it loads and when it can't be read.
pub fn texture(ctx: &eframe::egui::Context, abs: &Path) -> Option<eframe::egui::TextureHandle> {
    picture(ctx, abs).ready()
}

/// An image widget for a loaded picture, fitted into max_w × max_h.
pub fn fitted(tex: &eframe::egui::TextureHandle, max_w: f32, max_h: f32) -> eframe::egui::Image<'static> {
    eframe::egui::Image::from_texture(eframe::egui::load::SizedTexture::from_handle(tex)).max_width(max_w).max_height(max_h)
}

/// An image widget for a picture file, fitted into max_w × max_h; None while it loads and when it can't be read.
pub fn image_widget(ctx: &eframe::egui::Context, abs: &Path, max_w: f32, max_h: f32) -> Option<eframe::egui::Image<'static>> {
    texture(ctx, abs).map(|tex| fitted(&tex, max_w, max_h))
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
/// The file name is portable (VAULT-027) and at most 127 bytes with its date prefix and number.
fn free_media_path(vault_root: &Path, name: &str, date: (i32, u32, u32)) -> io::Result<PathBuf> {
    // names have to work inside [[links]]
    let safe = crate::markdown::link_safe_name(name);
    let prefix = media_rel_path("", date).file_name().map(|n| n.len()).unwrap_or(0);
    let room = crate::fsutil::MADE_NAME_MAX - prefix;
    let name = crate::fsutil::portable_name(&safe, room);
    let name = if name.trim().is_empty() { "file".to_string() } else { name };
    let mut rel = media_rel_path(&name, date);
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = Path::new(&name).extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let mut i = 1;
    while vault_root.join(&rel).exists() {
        // the number is part of the 127 bytes
        let suffix = format!(" {i}");
        let stem = crate::fsutil::portable_name(&stem, room.saturating_sub(ext.len() + suffix.len()));
        rel = media_rel_path(&format!("{stem}{suffix}{ext}"), date);
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

/// The markdown to insert: images, sounds and videos are embedded `![[…]]` (so they show a picture or
/// a player, MEDIA-001), other files linked `[[…]]`.
pub fn link_for(rel: &Path) -> String {
    let s = crate::vault::rel_to_string(rel);
    if is_image(rel) || is_media(rel) {
        format!("![[{s}]]")
    } else {
        format!("[[{s}]]")
    }
}

#[cfg(test)]
mod tests {
    // spec: EDIT-019
    #[test]
    fn only_a_lone_img_tag_is_a_browser_picture_paste() {
        assert!(super::is_lone_img_tag("<img src=\"image.png\">"));
        assert!(super::is_lone_img_tag("  <IMG src=\"a.png\" alt=\"x\"/>\n"));
        assert!(super::is_lone_img_tag(" <meta http-equiv=\"content-type\" content=\"text/html; charset=utf-8\"><img width=\"30\" src=\"https://x/logo.svg\" alt=\"Logo\" aria-hidden=\"true\">"));
        assert!(!super::is_lone_img_tag("<meta charset=\"utf-8\">"));
        assert!(!super::is_lone_img_tag("<img>"));
        assert!(!super::is_lone_img_tag("text <img src=\"a.png\">"));
        assert!(!super::is_lone_img_tag("<img src=\"a.png\"><img src=\"b.png\">"));
        assert!(!super::is_lone_img_tag("<imgx src=\"a.png\">"));
    }

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
        // spec: MEDIA-001
        assert_eq!(link_for(Path::new("z_media/a b.MP3")), "![[z_media/a b.MP3]]");
        assert_eq!(link_for(Path::new("clip.mp4")), "![[clip.mp4]]");
        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&vault);
    }

    // spec: MEDIA-019, EDIT-019
    #[test]
    fn pasted_files_are_copied_or_linked_and_what_cannot_be_copied_is_named() {
        let src = tmp("paste-src");
        let vault = tmp("paste-vault");
        fs::write(src.join("a.png"), b"p").unwrap();
        fs::write(src.join("b b.mp4"), b"v").unwrap();
        let list = format!("{}\n{}", src.join("a.png").display(), src.join("b b.mp4").display());
        let date = (2026, 11, 22);
        let (links, msg) = paste_text_to_links(&vault, &[], &list, intake::Choice::Copy, date).unwrap();
        assert_eq!((links.as_str(), msg.as_str()), ("![[z_media/2026/11/261122_a.png]]\n![[z_media/2026/11/261122_b b.mp4]]", "Saved 2 files to the media folder"));
        let abs = crate::fsutil::plain_path(src.canonicalize().unwrap());
        let (links, msg) = paste_text_to_links(&vault, &[], &list, intake::Choice::Link, date).unwrap();
        assert_eq!(links, format!("![](<{}>)\n![](<{}>)", intake::file_uri(&abs.join("a.png")), intake::file_uri(&abs.join("b b.mp4"))));
        assert_eq!(msg, "Linked 2 files where they are");
        let (_, msg) = paste_text_to_links(&vault, &[], &src.join("a.png").to_string_lossy(), intake::Choice::Link, date).unwrap();
        assert_eq!(msg, "Linked 1 file where it is");
        // a vault that cannot take a copy: the file is named; a link needs no copy
        let blocked = src.join("a.png");
        let (links, msg) = paste_text_to_links(&blocked, &[], &src.join("b b.mp4").to_string_lossy(), intake::Choice::Copy, date).unwrap();
        assert!(links.is_empty() && msg.starts_with("could not copy “b b.mp4”: "), "{msg}");
        assert!(paste_text_to_links(&blocked, &[], &src.join("b b.mp4").to_string_lossy(), intake::Choice::Link, date).unwrap().1.starts_with("Linked 1 file"));
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&vault);
    }

    // spec: EDIT-003
    #[test]
    fn local_date_is_sane() {
        let (y, m, d) = local_date();
        assert!(y >= 2024 && (1..=12).contains(&m) && (1..=31).contains(&d));
    }

    // spec: VAULT-027
    #[test]
    fn saved_media_names_are_portable_within_127_bytes_and_the_link_has_the_real_name() {
        let root = tmp("portable");
        let date = (2026, 11, 17);
        let rel = save_bytes(&root, "Plan: v2? <final>.pdf", b"x", date).unwrap();
        assert_eq!(crate::vault::rel_to_string(&rel), "z_media/2026/11/261117_Plan- v2- -final-.pdf");
        assert!(link_for(&rel).contains("261117_Plan- v2- -final-.pdf"), "the link uses the name the file got");
        let rel = save_bytes(&root, "NUL.txt", b"x", date).unwrap();
        assert!(rel.ends_with("261117_NUL_.txt"), "a device name gets its underscore: {rel:?}");
        let rel = save_bytes(&root, "name...", b"x", date).unwrap();
        assert!(rel.ends_with("261117_name"), "{rel:?}");
        // long names keep their extension and a number fits as well
        let long = format!("{}.pdf", "ä".repeat(200));
        let (a, b) = (save_bytes(&root, &long, b"1", date).unwrap(), save_bytes(&root, &long, b"2", date).unwrap());
        for r in [&a, &b] {
            let n = r.file_name().unwrap().to_string_lossy().into_owned();
            assert!(n.len() <= 127 && n.ends_with(".pdf"), "{n} is {} bytes", n.len());
        }
        assert_ne!(a, b);
        assert!(b.file_name().unwrap().to_string_lossy().ends_with(" 1.pdf"));
        // a file from elsewhere is copied under a portable name too
        let src = tmp("portable-src").join("Scan: 1?.png");
        fs::write(&src, b"png").unwrap();
        let rel = import_file(&root, &src, date).unwrap();
        assert_eq!(crate::vault::rel_to_string(&rel), "z_media/2026/11/261117_Scan- 1-.png");
        assert!(root.join(&rel).exists());
        let _ = fs::remove_dir_all(&root);
    }
}
