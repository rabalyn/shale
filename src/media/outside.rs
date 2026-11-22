//! Media files outside the vault (ADR-0030, MEDIA-014…MEDIA-018): a link names a picture, PDF,
//! sound or video that stays where it is. Two forms of link:
//!
//! - a **library link**, `![[@Videos/2024/a.mp4]]`: `Videos` is a library folder that each device
//!   maps to a local folder in its global settings (`media_libraries`), so the link means the same
//!   file on every device;
//! - a **file link**, `![](<file:///home/ada/Videos/a.mp4>)` (Obsidian's form, `%20` and the
//!   Windows form `file:///C:/…` too): one file, where it is on this computer.
//!
//! Only media kinds ([`super::media_kind`]) are outside links, and only to show: the paths found
//! here go to the players, the picture loader and `opener`, never to code that writes files
//! (VAULT-018). Everything in this file is pure but [`resolve`], which asks the disk and is for jobs
//! (exports, copy-in, the vault check), and [`look`], which frames use: the disk is asked on a
//! worker and the answer is kept (MEDIA-024).

use super::{file_uri_path, media_kind, Kind};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// What the caption or name of an outside file says on hover (MEDIA-023).
pub const NOT_SYNCED: &str = "Outside the vault: not synced";

/// The action that copies an outside file into the vault and links the copy (MEDIA-021).
pub const COPY_LABEL: &str = "Copy into the vault";

/// A library folder of this device: the name links use (`@Videos/…`) and the local folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub name: String,
    pub path: PathBuf,
}

impl Library {
    /// What is wrong with `name` as the name of a library, in a sentence; None when it works. A name
    /// is one part of a link (`@Name/…`), so it is not empty, has no `/` and none of the characters
    /// that break a `[[link]]` (`#`, `|`, `^`, brackets), works on Windows too, and is not the name
    /// of another library (any case). `others`: the names of the other libraries.
    pub fn name_problem<'a>(name: &str, mut others: impl Iterator<Item = &'a str>) -> Option<String> {
        if name.trim().is_empty() {
            return Some("Give the library a name".into());
        }
        if name.contains('/') {
            return Some("A name has no / in it".into());
        }
        if crate::markdown::link_safe_name(name) != name {
            return Some("A name has none of # | ^ [ ], no double spaces and none at its ends".into());
        }
        if let Some(p) = crate::fsutil::portable_problems(name, crate::fsutil::MADE_NAME_MAX).into_iter().next() {
            return Some(format!("Not a name that works everywhere: {p}"));
        }
        others.any(|o| o.eq_ignore_ascii_case(name)).then(|| "Another library has this name".into())
    }

    /// A library that links can be written with: a name that is fine and a folder.
    pub(crate) fn usable(&self) -> bool {
        Self::name_problem(&self.name, std::iter::empty()).is_none() && !self.path.as_os_str().is_empty()
    }
}

/// What a link target says, before anything is looked up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    /// `@Name/rest`: the part after the library's name, as written.
    Library { name: String, rest: String },
    /// `file://…`: the path on this computer.
    File(PathBuf),
}

/// Where a link target points when it is not a note: the library link or file link it is, if it
/// names a media file (by its extension). Any other target is None, and stays what it was: a note,
/// a file of the vault, or a red link.
pub fn spot(target: &str) -> Option<Spot> {
    let t = target.trim();
    if let Some(rest) = t.strip_prefix('@') {
        let (name, rest) = rest.split_once('/')?;
        return (!name.is_empty() && media_kind(Path::new(rest)).is_some()).then(|| Spot::Library { name: name.to_string(), rest: rest.to_string() });
    }
    let path = file_uri_path(t)?;
    media_kind(&path).is_some().then_some(Spot::File(path))
}

/// Whether `target` is a file link to a media file (`file://…`): the only address with `://` that
/// is a link of the vault's notes (`markdown::find_links`).
pub fn is_file_link(target: &str) -> bool {
    matches!(spot(target), Some(Spot::File(_)))
}

/// What an outside link points to on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outside {
    /// The file is there.
    Found { abs: PathBuf, kind: Kind },
    /// The path is clear but nothing is there (a moved disk, a deleted file).
    Missing { abs: PathBuf },

    /// A library link whose library this device has not set up.
    NoLibrary(String),
    /// A library link whose path leaves the library folder (`..`, a drive, an absolute path).
    LeavesLibrary { name: String },
}

impl Outside {
    /// What an embed or tooltip says when there is nothing to show; None for a file that is there.
    pub fn notice(&self) -> Option<String> {
        match self {
            Outside::Found { .. } => None,
            Outside::Missing { abs } => Some(format!("Not found: {}", abs.display())),
            Outside::NoLibrary(name) => Some(format!("Library “{name}” is not set up on this device")),
            Outside::LeavesLibrary { name } => Some(format!("This link leaves the library folder “{name}”")),
        }
    }
}

/// The path inside `library` that `rest` names, or None when it would leave it: only plain names
/// count (no `..`, no `.`, no drive or root), empty parts (`a//b`) are skipped.
fn inside(library: &Path, rest: &str) -> Option<PathBuf> {
    let mut abs = library.to_path_buf();
    for part in rest.split('/').filter(|p| !p.is_empty() && *p != ".") {
        let mut parts = Path::new(part).components();
        match (parts.next(), parts.next()) {
            (Some(Component::Normal(name)), None) => abs.push(name),
            _ => return None,
        }
    }
    Some(abs)
}

/// What a link names on this device before the disk is asked: the file and its kind, or what is
/// wrong already (a library that is not set up, a path that leaves it, a file of no media kind).
/// Library names match in any case; a library with an empty folder is not set up.
fn target(spot: &Spot, libraries: &[Library]) -> Result<(PathBuf, Kind), Outside> {
    let abs = match spot {
        Spot::File(path) => path.clone(),
        Spot::Library { name, rest } => {
            let Some(lib) = libraries.iter().find(|l| l.name.eq_ignore_ascii_case(name) && !l.path.as_os_str().is_empty()) else { return Err(Outside::NoLibrary(name.clone())) };
            inside(&lib.path, rest).ok_or_else(|| Outside::LeavesLibrary { name: name.clone() })?
        }
    };
    match media_kind(&abs) {
        Some(kind) => Ok((abs, kind)),
        None => Err(Outside::Missing { abs }),
    }
}

/// Look an outside link up on this device, asking the disk now: for jobs and tests. A frame must
/// not call this (a slow drive would freeze the window): it calls [`look`].
pub fn resolve(spot: &Spot, libraries: &[Library]) -> Outside {
    match target(spot, libraries) {
        Ok((abs, kind)) if abs.is_file() => Outside::Found { abs, kind },
        Ok((abs, _)) => Outside::Missing { abs },
        Err(outside) => outside,
    }
}

/// What a frame knows about an outside link (MEDIA-024).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    /// The path is clear and a worker is looking whether the file is there: the frame shows a quiet
    /// "loading" state of a stable size and the answer comes in a moment.
    Checking {
        abs: PathBuf,
        kind: Kind,
    },
    Is(Outside),
}

/// Look an outside link up for a frame (MEDIA-024): the answer of the last check of the file, which
/// a worker makes and repeats every few seconds, or `Checking` while the first one is on its way.
/// Never waits for the disk.
pub fn look(ctx: &eframe::egui::Context, spot: &Spot, libraries: &[Library]) -> Seen {
    match target(spot, libraries) {
        Ok((abs, kind)) => looked(ctx, abs, kind),
        Err(outside) => Seen::Is(outside),
    }
}

/// The answer of [`look`] for a file whose path and kind are known already (the link preview asks
/// again in later frames for a file that was `Checking`).
pub fn looked(ctx: &eframe::egui::Context, abs: PathBuf, kind: Kind) -> Seen {
    match super::look::Looks::of(ctx).facts(ctx, &abs) {
        super::look::Look::Checking => Seen::Checking { abs, kind },
        super::look::Look::Known(Some(f)) if f.is_file => Seen::Is(Outside::Found { abs, kind }),
        super::look::Look::Known(_) => Seen::Is(Outside::Missing { abs }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str, path: &str) -> Library {
        Library { name: name.into(), path: PathBuf::from(path) }
    }

    fn library(name: &str, rest: &str) -> Spot {
        Spot::Library { name: name.into(), rest: rest.into() }
    }

    // spec: MEDIA-014
    #[test]
    fn a_library_link_is_an_at_sign_a_name_and_a_media_file() {
        assert_eq!(spot("@Videos/2024/a.mp4"), Some(library("Videos", "2024/a.mp4")));
        assert_eq!(spot("  @Videos/doc.PDF "), Some(library("Videos", "doc.PDF")), "any case of the extension, trimmed");
        assert_eq!(spot("@Pictures/My Holiday/a b.jpg"), Some(library("Pictures", "My Holiday/a b.jpg")));
        for not in ["@Videos", "@Videos/", "@/a.mp4", "@Videos/notes.docx", "@Videos/a", "@Videos/2024/", "Videos/a.mp4", "a.mp4", "@work/Plan", ""] {
            assert_eq!(spot(not), None, "{not:?} is no library link");
        }
    }

    // spec: MEDIA-015
    #[test]
    fn a_file_link_is_a_file_address_of_a_media_file() {
        assert_eq!(spot("file:///home/ada/Videos/a.mp4"), Some(Spot::File(PathBuf::from("/home/ada/Videos/a.mp4"))));
        assert_eq!(spot("file:///home/ada/My%20Videos/a%20b.MKV"), Some(Spot::File(PathBuf::from("/home/ada/My Videos/a b.MKV"))), "%20 and any case");
        assert_eq!(spot("file:///home/ada/My Videos/a.mp4"), Some(Spot::File(PathBuf::from("/home/ada/My Videos/a.mp4"))), "a space written as it is");
        assert_eq!(spot("file://localhost/srv/a.wav"), Some(Spot::File(PathBuf::from("/srv/a.wav"))));
        assert_eq!(spot("file:///home/ada/100%25.png"), Some(Spot::File(PathBuf::from("/home/ada/100%.png"))), "decoded once");
        for not in ["file:///home/ada/notes.docx", "file:///home/ada/", "file://otherhost/a.mp4", "file://a.mp4", "https://example.org/a.mp4", "ftp://x/a.mp4"] {
            assert_eq!(spot(not), None, "{not:?} is no file link");
        }
        assert!(is_file_link("file:///x/a.mp3") && !is_file_link("@Music/a.mp3") && !is_file_link("a.mp3"));
    }

    // spec: MEDIA-015
    #[test]
    fn the_windows_form_of_a_file_address_is_read_on_every_system() {
        use crate::media::file_uri_path_for;
        let uri = "file:///C:/Users/Ada/My%20Videos/a.mp4";
        assert_eq!(file_uri_path_for(uri, true), Some(PathBuf::from(r"C:\Users\Ada\My Videos\a.mp4")), "Windows: the drive stands first, and the separators are those of Windows");
        assert_eq!(file_uri_path_for("file:////nas/share/a.mp4", true), Some(PathBuf::from(r"\\nas\share\a.mp4")), "a network path");
        assert_eq!(file_uri_path_for(uri, false), Some(PathBuf::from("/C:/Users/Ada/My Videos/a.mp4")), "elsewhere it is a folder called C:");
        assert_eq!(file_uri_path_for("file:///usr/a.mp4", true), Some(PathBuf::from(r"\usr\a.mp4")));
        // whichever system this is, the link is a file link
        assert!(matches!(spot(uri), Some(Spot::File(_))));
    }

    // spec: MEDIA-014
    #[test]
    fn a_library_path_stays_inside_its_folder() {
        let root = Path::new("/media/Videos");
        assert_eq!(inside(root, "2024/a.mp4"), Some(PathBuf::from("/media/Videos/2024/a.mp4")));
        assert_eq!(inside(root, "./2024//a.mp4"), Some(PathBuf::from("/media/Videos/2024/a.mp4")), "dots and empty parts are skipped");
        assert_eq!(inside(root, "/etc/a.mp4"), Some(PathBuf::from("/media/Videos/etc/a.mp4")), "a second slash does not make it absolute");
        for out in ["../a.mp4", "2024/../../a.mp4", "2024/../a.mp4", "..", "a/.."] {
            assert_eq!(inside(root, out), None, "{out:?} leaves the folder");
        }
        #[cfg(windows)]
        {
            for out in ["C:/a.mp4", "a\\..\\..\\b.mp4", "\\\\host\\share\\a.mp4"] {
                assert_eq!(inside(root, out), None, "{out:?} leaves the folder");
            }
        }
    }

    fn files(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("shale-outside-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let library = dir.join("Videos");
        std::fs::create_dir_all(library.join("2024")).unwrap();
        std::fs::write(library.join("2024/a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("secret.mp4"), b"x").unwrap();
        (dir, library)
    }

    // spec: MEDIA-014, MEDIA-015
    #[test]
    fn an_outside_link_is_found_missing_unmapped_or_refused() {
        let (dir, library_dir) = files("resolve");
        let libs = [lib("Videos", &library_dir.to_string_lossy()), lib("Empty", "")];
        let found = resolve(&library("videos", "2024/a.mp4"), &libs);
        assert_eq!(found, Outside::Found { abs: library_dir.join("2024/a.mp4"), kind: Kind::Video }, "the name matches in any case");
        assert_eq!(found.notice(), None);
        // nothing there: the path is told
        let missing = resolve(&library("Videos", "2024/b.mp4"), &libs);
        assert_eq!(missing, Outside::Missing { abs: library_dir.join("2024/b.mp4") });
        assert_eq!(missing.notice(), Some(format!("Not found: {}", library_dir.join("2024").join("b.mp4").display())));
        // a folder of that name is not a file
        std::fs::create_dir_all(library_dir.join("dir.mp4")).unwrap();
        assert!(matches!(resolve(&library("Videos", "dir.mp4"), &libs), Outside::Missing { .. }));
        // a library this device has not set up, or without a folder
        for name in ["Music", "Empty"] {
            let unmapped = resolve(&library(name, "a.mp3"), &libs);
            assert_eq!(unmapped, Outside::NoLibrary(name.into()));
            assert_eq!(unmapped.notice(), Some(format!("Library “{name}” is not set up on this device")));
        }
        assert!(matches!(resolve(&library("Music", "a.mp3"), &[]), Outside::NoLibrary(_)));
        // the file that is there, one level up, is not reached
        let escaped = resolve(&library("Videos", "../secret.mp4"), &libs);
        assert_eq!(escaped, Outside::LeavesLibrary { name: "Videos".into() });
        assert!(escaped.notice().unwrap().contains("leaves the library folder"));
        // a file link: the path as it is
        assert_eq!(resolve(&Spot::File(dir.join("secret.mp4")), &[]), Outside::Found { abs: dir.join("secret.mp4"), kind: Kind::Video });
        assert!(matches!(resolve(&Spot::File(dir.join("gone.mp4")), &[]), Outside::Missing { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-014
    #[test]
    fn a_non_media_path_is_never_found_even_when_the_file_is_there() {
        let (dir, library_dir) = files("kind");
        std::fs::write(library_dir.join("notes.txt"), b"x").unwrap();
        let libs = [lib("Videos", &library_dir.to_string_lossy())];
        assert_eq!(spot("@Videos/notes.txt"), None, "it is no link of this kind at all");
        // and a spot made by hand is still not found
        assert!(matches!(resolve(&library("Videos", "notes.txt"), &libs), Outside::Missing { .. }));
        assert!(matches!(resolve(&Spot::File(library_dir.join("notes.txt")), &[]), Outside::Missing { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-019, MEDIA-014
    #[test]
    fn a_library_name_is_one_portable_part_of_a_link_and_not_taken() {
        let ok = |n: &str, others: &[&str]| Library::name_problem(n, others.iter().copied());
        for fine in ["Videos", "Meine Filme", "Übungen 2024", "a-b_c.d", "@x"] {
            assert_eq!(ok(fine, &["Music"]), None, "{fine:?}");
        }
        assert_eq!(ok("", &[]).as_deref(), Some("Give the library a name"));
        assert_eq!(ok("   ", &[]).as_deref(), Some("Give the library a name"));
        assert_eq!(ok("a/b", &[]).as_deref(), Some("A name has no / in it"));
        for bad in ["a#b", "a|b", "a^b", "a[b", "a]b", "a  b", " a", "a "] {
            assert!(ok(bad, &[]).is_some_and(|p| p.starts_with("A name has none of")), "{bad:?}: {:?}", ok(bad, &[]));
        }
        for bad in ["a:b", "a*b", "CON", "name."] {
            assert!(ok(bad, &[]).is_some_and(|p| p.starts_with("Not a name that works everywhere")), "{bad:?}: {:?}", ok(bad, &[]));
        }
        assert_eq!(ok("videos", &["Music", "Videos"]).as_deref(), Some("Another library has this name"), "any case");
        assert_eq!(ok("Videos", &[]), None, "itself is not another one");
        // what links are written with: a good name and a folder
        assert!(lib("Videos", "/v").usable() && !lib("Videos", "").usable() && !lib("a|b", "/v").usable() && !lib("", "/v").usable());
    }

    // spec: CFG-069, CFG-001
    #[test]
    fn a_library_reads_with_missing_fields() {
        let l: Library = serde_json::from_str(r#"{"name": "Videos"}"#).unwrap();
        assert_eq!(l, lib("Videos", ""));
        let back: Library = serde_json::from_str(&serde_json::to_string(&lib("A", "/x/y")).unwrap()).unwrap();
        assert_eq!(back, lib("A", "/x/y"));
    }
}
