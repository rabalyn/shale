//! What happens to a file from outside the vault when it is picked, pasted or imported (MEDIA-019,
//! MEDIA-020, ADR-0030): it is copied into `z_media/` (EDIT-003), or, when it is a picture, PDF,
//! sound or video, linked where it is. Every entry point calls [`bring`] with its [`Choice`]; the
//! setting ([`OutsideFiles`]) only decides which choice an entry point makes, or that it asks.
//!
//! Only these four kinds can be outside links, so any other file from outside is always copied. A
//! file that already lies in the vault is linked in place whatever the choice. A download (a social
//! post's pictures, a mail attachment, a picture of a Word file) is no file from outside and does
//! not come here.

use super::outside::Library;
use super::{import_file, is_image, is_media, media_kind};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The setting "Files from outside the vault" (MEDIA-019, CFG-070).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OutsideFiles {
    /// the entry point asks (the Import dialog is its own question, with Copy preset)
    #[default]
    Ask,
    Copy,
    Link,
}

impl OutsideFiles {
    pub const ALL: [OutsideFiles; 3] = [OutsideFiles::Ask, OutsideFiles::Copy, OutsideFiles::Link];

    pub fn label(self) -> &'static str {
        match self {
            OutsideFiles::Ask => "Ask",
            OutsideFiles::Copy => "Copy into the vault",
            OutsideFiles::Link => "Link where they are",
        }
    }

    /// What the setting does, for the tooltip of its button in Settings.
    pub fn tip(self) -> &'static str {
        match self {
            OutsideFiles::Ask => "Every time, a small question: copy it or link it. In the Import dialog each file has the choice itself",
            OutsideFiles::Copy => "A copy goes to the media folder (z_media/) of the vault and is synced with it",
            OutsideFiles::Link => "The note links the file where it is: nothing is copied, and the file is not synced with the vault",
        }
    }

    /// The choice the setting makes by itself; None for Ask.
    pub fn choice(self) -> Option<Choice> {
        match self {
            OutsideFiles::Ask => None,
            OutsideFiles::Copy => Some(Choice::Copy),
            OutsideFiles::Link => Some(Choice::Link),
        }
    }

    /// The choice to start from where the question is part of the screen (the Import dialog).
    pub fn preset(self) -> Choice {
        self.choice().unwrap_or_default()
    }
}

/// What is done with one file from outside the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    /// a copy in `z_media/`
    #[default]
    Copy,
    /// a link to the file where it is
    Link,
}

impl Choice {
    pub fn toggled(self) -> Choice {
        match self {
            Choice::Copy => Choice::Link,
            Choice::Link => Choice::Copy,
        }
    }
}

/// What [`bring`] did with a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Taken {
    /// it was in the vault already: the vault path
    InVault(PathBuf),
    /// copied into the vault: the path of the copy
    Copied(PathBuf),
    /// left where it is: the link text and the file
    Linked { link: String, path: PathBuf },
}

impl Taken {
    /// The markdown to insert.
    pub fn link(&self) -> String {
        match self {
            Taken::InVault(rel) | Taken::Copied(rel) => super::link_for(rel),
            Taken::Linked { link, .. } => link.clone(),
        }
    }
}

/// Bring `src` into a note: a file of the vault is linked in place; another file is linked where
/// it is when `how` says so and it is a picture, PDF, sound or video; any other file is copied to
/// the media folder (a number is added if the name is taken).
pub fn bring(vault_root: &Path, libraries: &[Library], src: &Path, how: Choice, date: (i32, u32, u32)) -> io::Result<Taken> {
    let root = vault_root.canonicalize().unwrap_or_else(|_| vault_root.to_path_buf());
    let file = src.canonicalize()?;
    if let Ok(rel) = file.strip_prefix(&root) {
        return Ok(Taken::InVault(rel.to_path_buf()));
    }
    if how == Choice::Link && media_kind(&file).is_some() {
        let libraries: Vec<Library> = libraries.iter().map(|l| Library { name: l.name.clone(), path: l.path.canonicalize().map(plain).unwrap_or_else(|_| l.path.clone()) }).collect();
        let file = plain(file);
        return Ok(Taken::Linked { link: link_text(&file, &libraries), path: file });
    }
    import_file(vault_root, src, date).map(Taken::Copied)
}

/// Whether one of `files` is a picture, PDF, sound or video outside the vault: a file that could be
/// linked, so that the setting Ask has something to ask about.
pub fn asks(vault_root: &Path, files: &[PathBuf]) -> bool {
    let root = vault_root.canonicalize().unwrap_or_else(|_| vault_root.to_path_buf());
    files.iter().any(|f| media_kind(f).is_some() && f.canonicalize().is_ok_and(|c| !c.starts_with(&root)))
}

/// A path as Windows programs write it: `canonicalize` makes `\\?\C:\x` of `C:\x`.
fn plain(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        if let Some(p) = without_verbatim(&path.to_string_lossy()) {
            return PathBuf::from(p);
        }
    }
    path
}

/// `C:\x` for `\\?\C:\x` and `\\server\share\x` for `\\?\UNC\server\share\x`; None for any other path.
fn without_verbatim(p: &str) -> Option<String> {
    if let Some(unc) = p.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{unc}"));
    }
    p.strip_prefix(r"\\?\").map(str::to_string)
}

/// The text that links `path`, a picture, PDF, sound or video outside the vault (MEDIA-020): a
/// **library link** `![[@Name/rest]]` when the file lies under the folder of a library (the longest
/// folder wins; the first library of a name, as `outside::resolve` finds it), otherwise a **file
/// link** `![](<file:///…>)`. A file that is not embedded (a PDF) is `[[@Name/rest]]` or
/// `[name](<file:///…>)`. A library link is also left out when a part of the path would break a
/// `[[link]]` (`#`, `|`, `^`, brackets): the file link has no such limit. Paths are compared as they
/// are, part by part; the caller makes them canonical.
pub fn link_text(path: &Path, libraries: &[Library]) -> String {
    let embed = is_image(path) || is_media(path);
    if let Some((name, rest)) = library_path(path, libraries) {
        return format!("{}[[@{name}/{rest}]]", if embed { "!" } else { "" });
    }
    let uri = file_uri_for(&path.to_string_lossy(), cfg!(windows));
    if embed {
        format!("![](<{uri}>)")
    } else {
        let label: String = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c == '[' || c == ']' { '-' } else { c }).collect();
        format!("[{label}](<{uri}>)")
    }
}

/// The library `path` lies in and the rest of the path after its folder, as a link writes it.
fn library_path<'a>(path: &Path, libraries: &'a [Library]) -> Option<(&'a str, String)> {
    let mut best: Option<(usize, &str, String)> = None;
    for (i, lib) in libraries.iter().enumerate() {
        let shadowed = libraries[..i].iter().any(|e| e.name.eq_ignore_ascii_case(&lib.name) && !e.path.as_os_str().is_empty());
        if !lib.usable() || shadowed {
            continue;
        }
        let Ok(under) = path.strip_prefix(&lib.path) else { continue };
        let parts: Option<Vec<&str>> = under.components().map(|c| if let std::path::Component::Normal(n) = c { n.to_str() } else { None }).collect();
        let Some(parts) = parts.filter(|p| !p.is_empty() && p.iter().all(|s| crate::markdown::link_safe_name(s) == *s)) else { continue };
        let depth = lib.path.components().count();
        if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
            best = Some((depth, lib.name.as_str(), parts.join("/")));
        }
    }
    best.map(|(_, name, rest)| (name, rest))
}

/// The `file://` address of a path as the markdown link `![](<…>)` holds it, the form that
/// `file_uri_path_for` and Obsidian read back: spaces and other letters stand as they are, and
/// `%`, `)`, `<`, `>` and control characters are written `%XX`. `windows`: `C:\Users\ada\a.mp4` is
/// `file:///C:/Users/ada/a.mp4` (a network path `\\host\share` is `file:////host/share`).
fn file_uri_for(path: &str, windows: bool) -> String {
    let p = if windows { path.replace('\\', "/") } else { path.to_string() };
    let mut uri = String::from("file://");
    if !p.starts_with('/') {
        uri.push('/');
    }
    for c in p.chars() {
        if matches!(c, '%' | ')' | '<' | '>') || c.is_control() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                uri.push_str(&format!("%{b:02X}"));
            }
        } else {
            uri.push(c);
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::find_links;
    use crate::media::outside::{self, Outside};
    use std::fs;

    fn lib(name: &str, path: &str) -> Library {
        Library { name: name.into(), path: PathBuf::from(path) }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-intake-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: MEDIA-019, CFG-070
    #[test]
    fn the_setting_asks_by_default_and_the_other_two_choose_for_themselves() {
        assert_eq!(OutsideFiles::default(), OutsideFiles::Ask);
        assert_eq!(OutsideFiles::ALL.map(OutsideFiles::choice), [None, Some(Choice::Copy), Some(Choice::Link)]);
        assert_eq!(OutsideFiles::ALL.map(OutsideFiles::preset), [Choice::Copy, Choice::Copy, Choice::Link], "where the question is on the screen, Ask starts at Copy");
        assert_eq!(OutsideFiles::ALL.map(OutsideFiles::label), ["Ask", "Copy into the vault", "Link where they are"]);
        assert_eq!(Choice::Copy.toggled(), Choice::Link);
        assert_eq!(Choice::Link.toggled(), Choice::Copy);
    }

    // spec: MEDIA-020
    #[test]
    fn a_file_under_a_library_folder_is_a_library_link_with_slashes_and_the_longest_folder_wins() {
        let libs = [lib("Videos", "/media/ada/Videos"), lib("Trips", "/media/ada/Videos/Trips"), lib("Nothing", "")];
        assert_eq!(link_text(Path::new("/media/ada/Videos/2024/a b.mp4"), &libs), "![[@Videos/2024/a b.mp4]]");
        assert_eq!(link_text(Path::new("/media/ada/Videos/Trips/Rome/a.mkv"), &libs), "![[@Trips/Rome/a.mkv]]", "the more exact folder");
        assert_eq!(link_text(Path::new("/media/ada/Videos/Tripsy/a.mkv"), &libs), "![[@Videos/Tripsy/a.mkv]]", "a folder name that only starts alike");
        assert_eq!(link_text(Path::new("/media/ada/Videos/doc.PDF"), &libs), "[[@Videos/doc.PDF]]", "a PDF is a plain link as in EDIT-003");
        assert_eq!(link_text(Path::new("/media/ada/Videos/p.png"), &libs), "![[@Videos/p.png]]");
        assert_eq!(link_text(Path::new("/media/ada/Videos/s.mp3"), &libs), "![[@Videos/s.mp3]]");
        assert_eq!(link_text(Path::new("/media/ada/Videos/old.avi"), &libs), "![[@Videos/old.avi]]", "an avi has a player like the other videos");
    }

    // spec: MEDIA-020
    #[test]
    fn a_file_that_is_in_no_library_is_a_file_link_and_so_is_one_a_wiki_link_could_not_hold() {
        let libs = [lib("Videos", "/media/ada/Videos"), lib("Unset", "")];
        assert_eq!(link_text(Path::new("/home/ada/My Videos/a.mp4"), &libs), "![](<file:///home/ada/My Videos/a.mp4>)");
        assert_eq!(link_text(Path::new("/home/ada/doc.pdf"), &libs), "[doc.pdf](<file:///home/ada/doc.pdf>)");
        assert_eq!(link_text(Path::new("/home/ada/[x].pdf"), &libs), "[-x-.pdf](<file:///home/ada/[x].pdf>)", "the label has no brackets, the address keeps the name");
        assert_eq!(link_text(Path::new("/home/ada/a.mp4"), &[]), "![](<file:///home/ada/a.mp4>)");
        // in a library, but a name that [[ ]] cannot hold: the file link says it exactly
        assert_eq!(link_text(Path::new("/media/ada/Videos/Q&A #2|b.mp4"), &libs), "![](<file:///media/ada/Videos/Q&A #2|b.mp4>)");
        assert_eq!(link_text(Path::new("/media/ada/Videos/a  b.mp4"), &libs), "![](<file:///media/ada/Videos/a  b.mp4>)", "two spaces would be one in a link");
        // a library with a bad name or no folder is not used, the first of a name is
        let libs = [lib("Bad|name", "/media/ada/Videos"), lib("Dup", "/a"), lib("dup", "/a/b")];
        assert_eq!(link_text(Path::new("/media/ada/Videos/a.mp4"), &libs), "![](<file:///media/ada/Videos/a.mp4>)");
        assert_eq!(link_text(Path::new("/a/b/c.mp4"), &libs), "![[@Dup/b/c.mp4]]", "the second library of the name is never looked up, so no link goes through it");
        // a folder of the library itself is no file in it
        assert_eq!(link_text(Path::new("/a"), &libs), "[a](<file:///a>)");
    }

    // spec: MEDIA-020
    #[test]
    fn the_file_address_encodes_what_would_end_the_link_and_has_the_windows_form() {
        assert_eq!(file_uri_for("/h/100%/a (1) <b>.mp4", false), "file:///h/100%25/a (1%29 %3Cb%3E.mp4", "only the closing parenthesis, not the opening one");
        assert_eq!(file_uri_for("/h/ü ñ/ä.mp4", false), "file:///h/ü ñ/ä.mp4", "letters and spaces stand as they are");
        assert_eq!(file_uri_for("/h/a\nb\u{85}.mp4", false), "file:///h/a%0Ab%C2%85.mp4", "controls, also those of two bytes");
        assert_eq!(file_uri_for(r"C:\Users\ada\My Videos\a.mp4", true), "file:///C:/Users/ada/My Videos/a.mp4");
        assert_eq!(file_uri_for(r"\\nas\share\a.mp4", true), "file:////nas/share/a.mp4");
        assert_eq!(without_verbatim(r"\\?\C:\Users\a.mp4").as_deref(), Some(r"C:\Users\a.mp4"));
        assert_eq!(without_verbatim(r"\\?\UNC\nas\share\a.mp4").as_deref(), Some(r"\\nas\share\a.mp4"));
        assert_eq!(without_verbatim(r"C:\Users\a.mp4"), None);
        // and the system's own reader gives the path back
        for (path, windows) in [(r"C:\Users\ada\My 100% (1) Videos\a.mp4", true), (r"\\nas\share\a.mp4", true), ("/h/a (1) 100%.mp4", false)] {
            let uri = file_uri_for(path, windows);
            let back = crate::media::file_uri_path_for(&uri, windows).unwrap();
            assert_eq!(back.to_string_lossy().replace('\\', "/"), path.replace('\\', "/"), "{uri}");
        }
    }

    // spec: MEDIA-020, MEDIA-014, MEDIA-015
    #[test]
    fn a_written_link_is_read_back_by_find_links_and_resolves_to_the_same_file() {
        let dir = tmp("roundtrip");
        let lib_dir = dir.join("Videos Library");
        fs::create_dir_all(lib_dir.join("2024")).unwrap();
        let other = dir.join("loose (copy) 100% <x>");
        fs::create_dir_all(&other).unwrap();
        let libs = [Library { name: "Videos".into(), path: lib_dir.clone() }];
        let mut files = vec![lib_dir.join("2024/a b.mp4"), lib_dir.join("doc.pdf"), lib_dir.join("pic.png"), other.join("clip (1) 100%.mp4"), other.join("Plan (2).pdf"), other.join("song.mp3")];
        files.push(dir.join("x y.png"));
        for f in &files {
            fs::write(f, b"x").unwrap();
            let text = format!("see {} here", link_text(f, &libs));
            let found = find_links(&text);
            assert_eq!(found.len(), 1, "{text}");
            let spot = outside::spot(&found[0].target).unwrap_or_else(|| panic!("{text}: no outside link"));
            match outside::resolve(&spot, &libs) {
                Outside::Found { abs, .. } => assert_eq!(abs, *f, "{text}"),
                other => panic!("{text}: {other:?}"),
            }
            assert_eq!(found[0].embed, is_image(f) || is_media(f), "{text}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-019, MEDIA-020, EDIT-003
    #[test]
    fn bring_links_in_place_copies_or_links_where_it_is_and_always_copies_other_files() {
        let dir = tmp("bring");
        let vault = dir.join("vault");
        let outside = dir.join("outside");
        let lib_dir = dir.join("lib");
        for d in [&vault, &outside, &lib_dir] {
            fs::create_dir_all(d).unwrap();
        }
        let libs = [Library { name: "Lib".into(), path: lib_dir.clone() }];
        let date = (2026, 11, 22);
        fs::write(vault.join("in.mp4"), b"v").unwrap();
        fs::write(outside.join("a.mp4"), b"v").unwrap();
        fs::write(lib_dir.join("b.png"), b"p").unwrap();
        fs::write(outside.join("notes.txt"), b"t").unwrap();
        // in the vault: linked in place, whatever the choice
        for how in [Choice::Copy, Choice::Link] {
            assert_eq!(bring(&vault, &libs, &vault.join("in.mp4"), how, date).unwrap(), Taken::InVault(PathBuf::from("in.mp4")));
        }
        // Copy: a copy in the media folder
        let copied = bring(&vault, &libs, &outside.join("a.mp4"), Choice::Copy, date).unwrap();
        assert_eq!(copied, Taken::Copied(PathBuf::from("z_media/2026/11/261122_a.mp4")));
        assert_eq!(copied.link(), "![[z_media/2026/11/261122_a.mp4]]");
        assert!(vault.join("z_media/2026/11/261122_a.mp4").is_file());
        // a file under a library folder is not linked by itself: Copy copies it
        assert!(matches!(bring(&vault, &libs, &lib_dir.join("b.png"), Choice::Copy, date).unwrap(), Taken::Copied(_)));
        // Link: nothing is written into the vault
        let before: Vec<_> = walk(&vault);
        let linked = bring(&vault, &libs, &outside.join("a.mp4"), Choice::Link, date).unwrap();
        let abs = outside.join("a.mp4").canonicalize().unwrap();
        assert_eq!(linked, Taken::Linked { link: format!("![](<file://{}>)", abs.display()), path: abs });
        let in_lib = bring(&vault, &libs, &lib_dir.join("b.png"), Choice::Link, date).unwrap();
        assert_eq!(in_lib.link(), "![[@Lib/b.png]]");
        assert_eq!(walk(&vault), before, "linking writes nothing");
        // other files are copied, also when Link was chosen
        let note = bring(&vault, &libs, &outside.join("notes.txt"), Choice::Link, date).unwrap();
        assert_eq!(note, Taken::Copied(PathBuf::from("z_media/2026/11/261122_notes.txt")));
        assert_eq!(note.link(), "[[z_media/2026/11/261122_notes.txt]]");
        // a file that is not there is an error, not a link
        assert!(bring(&vault, &libs, &outside.join("gone.mp4"), Choice::Link, date).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in fs::read_dir(dir).unwrap().flatten() {
            out.push(e.path());
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            }
        }
        out.sort();
        out
    }

    // spec: MEDIA-019
    #[test]
    fn only_a_media_file_outside_the_vault_is_something_to_ask_about() {
        let dir = tmp("asks");
        let vault = dir.join("vault");
        fs::create_dir_all(&vault).unwrap();
        for f in ["vault/in.mp4", "a.mp4", "n.txt"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        assert!(asks(&vault, &[dir.join("a.mp4")]));
        assert!(asks(&vault, &[vault.join("in.mp4"), dir.join("a.mp4")]), "one is enough");
        assert!(!asks(&vault, &[vault.join("in.mp4")]), "linked in place");
        assert!(!asks(&vault, &[dir.join("n.txt")]), "always copied");
        assert!(!asks(&vault, &[dir.join("gone.mp4")]), "not there: the copy will say so");
        assert!(!asks(&vault, &[]));
        let _ = fs::remove_dir_all(&dir);
    }
}
