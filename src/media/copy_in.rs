//! Copy into the vault (MEDIA-021, ADR-0030): the files that a note's outside links name are
//! copied to `z_media/…` (EDIT-003) and the links are rewritten to point at the copies.
//!
//! [`copy_into`] looks the files up and copies them, which touches the disks of the outside
//! files, so the window runs it in a background job (JOB-001); [`relink`] is pure text and is
//! applied to the note when the job has ended, whatever the note says by then: it finds the links
//! by their targets.

use super::import_file;
use super::outside::{resolve, spot, Library, Outside};
use crate::markdown;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

/// What [`copy_into`] did, by link target as written in the note.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copied {
    /// the targets whose file is now in the vault, with its path there
    pub done: Vec<(String, PathBuf)>,
    /// the targets whose file is not there (not found, library not set up, path out of the
    /// library): the links stay as they are
    pub missing: Vec<String>,
    /// the targets whose file could not be copied, with the reason
    pub failed: Vec<(String, String)>,
}

impl Copied {
    /// The message for the toast: how many files went into the vault and what was left.
    pub fn message(&self) -> String {
        let files = self.files();
        let mut out = match files {
            0 => "No file was copied into the vault".to_string(),
            1 => "Copied 1 file into the vault".to_string(),
            n => format!("Copied {n} files into the vault"),
        };
        if !self.missing.is_empty() {
            out.push_str(&format!(". Left as they are, the file is not there: {}", self.missing.join(", ")));
        }
        if !self.failed.is_empty() {
            let list: Vec<String> = self.failed.iter().map(|(t, e)| format!("{t} ({e})")).collect();
            out.push_str(&format!(". Could not copy: {}", list.join(", ")));
        }
        out
    }

    /// How many files were copied: links to one file are one copy.
    pub fn files(&self) -> usize {
        let mut seen: Vec<&PathBuf> = Vec::new();
        for (_, rel) in &self.done {
            if !seen.contains(&rel) {
                seen.push(rel);
            }
        }
        seen.len()
    }
}

/// Copy the files that the outside links `targets` name into the vault at `root` (a file that
/// several targets name is copied once). Only an outside link that resolves to a file that is there
/// is copied (`Outside::Found`); the rest is reported. Nothing outside the vault is changed.
pub fn copy_into(root: &std::path::Path, libraries: &[Library], targets: &[String], date: (i32, u32, u32)) -> Copied {
    let mut out = Copied::default();
    let mut copies: HashMap<PathBuf, PathBuf> = HashMap::new();
    for target in targets {
        let Some(spot) = spot(target) else { continue };
        match resolve(&spot, libraries) {
            Outside::Found { abs, .. } => {
                if let Some(rel) = copies.get(&abs) {
                    out.done.push((target.clone(), rel.clone()));
                    continue;
                }
                match import_file(root, &abs, date) {
                    Ok(rel) => {
                        copies.insert(abs, rel.clone());
                        out.done.push((target.clone(), rel));
                    }
                    Err(e) => out.failed.push((target.clone(), e.to_string())),
                }
            }
            _ => out.missing.push(target.clone()),
        }
    }
    out
}

/// A note's text after [`relink`], and what moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relinked {
    pub text: String,
    /// the targets that were replaced: where each was in the old text (chars) and how long its
    /// replacement is (chars), in the order of the text
    pub changes: Vec<(Range<usize>, usize)>,
}

/// `text` with every outside link whose target is in `done` pointing at its copy in the vault:
/// `![[@Videos/a.mp4|300]]` → `![[z_media/2026/11/261122_a.mp4|300]]`, a markdown link or image
/// likewise. Only the target changes; the embed mark, the alias or width, the heading and the
/// `<…>` of a markdown link stay. None when no link matches.
pub fn relink(text: &str, done: &[(String, PathBuf)]) -> Option<Relinked> {
    let mut out = text.to_string();
    let mut changes = Vec::new();
    for link in markdown::find_links(text).into_iter().rev() {
        let Some((_, rel)) = done.iter().find(|(t, _)| *t == link.target).filter(|_| link.is_outside()) else { continue };
        let new = markdown::replace_target(text, &link, &crate::vault::rel_to_string(rel));
        let chars = markdown::byte_to_char(text, link.target_range.start)..markdown::byte_to_char(text, link.target_range.end);
        changes.push((chars, new.chars().count()));
        out.replace_range(link.target_range.clone(), &new);
    }
    changes.reverse();
    (!changes.is_empty()).then_some(Relinked { text: out, changes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const DATE: (i32, u32, u32) = (2026, 11, 22);

    struct Dirs {
        root: PathBuf,
        vault: PathBuf,
        videos: PathBuf,
    }

    impl Dirs {
        fn new(tag: &str) -> Dirs {
            let root = std::env::temp_dir().join(format!("shale-copyin-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let (vault, videos) = (root.join("vault"), root.join("Videos"));
            fs::create_dir_all(&vault).unwrap();
            fs::create_dir_all(videos.join("2024")).unwrap();
            Dirs { root, vault, videos }
        }

        fn libraries(&self) -> Vec<Library> {
            vec![Library { name: "Videos".into(), path: self.videos.clone() }]
        }
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn done(t: &str, rel: &str) -> (String, PathBuf) {
        (t.to_string(), PathBuf::from(rel))
    }

    // spec: MEDIA-021
    #[test]
    fn the_files_of_outside_links_are_copied_once_and_what_is_not_there_is_reported() {
        let d = Dirs::new("copy");
        fs::write(d.videos.join("2024/a.mp4"), b"video a").unwrap();
        fs::write(d.videos.join("b.png"), b"picture").unwrap();
        let by_address = crate::media::testdata::file_address(&d.videos.join("2024/a.mp4"));
        let targets: Vec<String> = ["@Videos/2024/a.mp4", &by_address, "@Videos/b.png", "@Videos/gone.mp3", "@Nowhere/c.mp3", "@Videos/../x.png", "plain.png"].map(String::from).to_vec();
        let c = copy_into(&d.vault, &d.libraries(), &targets, DATE);
        assert_eq!(c.done.len(), 3);
        assert_eq!(c.done[0].1, c.done[1].1, "a library link and a file address of one file are one copy");
        assert_eq!(c.files(), 2);
        let rel_a = c.done[0].1.clone();
        assert!(rel_a.starts_with("z_media/2026/11") && rel_a.to_string_lossy().ends_with("_a.mp4"), "{rel_a:?}");
        assert_eq!(fs::read(d.vault.join(&rel_a)).unwrap(), b"video a");
        assert_eq!(fs::read(d.vault.join(&c.done[2].1)).unwrap(), b"picture");
        assert_eq!(c.missing, vec!["@Videos/gone.mp3", "@Nowhere/c.mp3", "@Videos/../x.png"], "a file that is not there, a library not set up, a path out of the library");
        assert!(c.failed.is_empty());
        assert!(d.videos.join("2024/a.mp4").is_file(), "the original stays where it is");
        let m = c.message();
        assert!(m.starts_with("Copied 2 files into the vault") && m.contains("@Videos/gone.mp3, @Nowhere/c.mp3"), "{m}");
        assert_eq!(Copied::default().message(), "No file was copied into the vault");
        let one = Copied { done: vec![done("a", "z_media/a.png")], ..Copied::default() };
        assert_eq!(one.message(), "Copied 1 file into the vault");
    }

    // spec: MEDIA-021
    #[test]
    fn a_file_that_cannot_be_copied_is_reported_and_its_link_is_left() {
        let d = Dirs::new("failed");
        fs::write(d.videos.join("a.mp4"), b"x").unwrap();
        // the media folder of the vault is a file: no copy can be made there
        fs::write(d.vault.join("z_media"), b"in the way").unwrap();
        let c = copy_into(&d.vault, &d.libraries(), &["@Videos/a.mp4".to_string()], DATE);
        assert!(c.done.is_empty() && c.missing.is_empty());
        assert_eq!(c.failed.len(), 1);
        assert_eq!(c.failed[0].0, "@Videos/a.mp4");
        assert!(c.message().contains("Could not copy: @Videos/a.mp4 ("), "{}", c.message());
    }

    // spec: MEDIA-021
    #[test]
    fn a_file_link_to_a_file_in_the_vault_becomes_a_link_of_the_vault() {
        let d = Dirs::new("inside");
        fs::create_dir_all(d.vault.join("pics")).unwrap();
        fs::write(d.vault.join("pics/a.png"), b"x").unwrap();
        let target = crate::media::testdata::file_address(&d.vault.join("pics/a.png"));
        let c = copy_into(&d.vault, &[], std::slice::from_ref(&target), DATE);
        assert_eq!(c.done, vec![(target, PathBuf::from("pics/a.png"))], "linked in place, not copied again");
        assert!(!d.vault.join("z_media").exists());
    }

    // spec: MEDIA-021
    #[test]
    fn the_link_text_keeps_its_form_width_alias_and_heading() {
        let text = "a ![[@Videos/a.mp4|300]] b [[@Videos/doc.pdf|the paper]] c ![alt](<file:///home/ada/My Videos/x.png>) d [t](@Videos/y.png) ![[@Videos/a.mp4]] ![[@Videos/doc.pdf#page=2]]";
        let all = [done("@Videos/a.mp4", "z_media/a.mp4"), done("@Videos/doc.pdf", "z_media/doc.pdf"), done("file:///home/ada/My Videos/x.png", "z_media/x y.png"), done("@Videos/y.png", "z_media/y.png")];
        let r = relink(text, &all).unwrap();
        assert_eq!(r.text, "a ![[z_media/a.mp4|300]] b [[z_media/doc.pdf|the paper]] c ![alt](<z_media/x y.png>) d [t](z_media/y.png) ![[z_media/a.mp4]] ![[z_media/doc.pdf#page=2]]");
        assert_eq!(r.changes.len(), 6);
        assert!(r.changes.windows(2).all(|w| w[0].0.end <= w[1].0.start), "in the order of the text");
        // the ranges are chars of the old text
        let first: String = text.chars().skip(r.changes[0].0.start).take(r.changes[0].0.len()).collect();
        assert_eq!(first, "@Videos/a.mp4");
        assert_eq!(r.changes[0].1, "z_media/a.mp4".chars().count());
    }

    // spec: MEDIA-021
    #[test]
    fn only_the_links_that_were_copied_change_and_a_note_without_them_stays() {
        let text = "![[@Videos/a.mp4]] ![[@Videos/b.mp4]] [[Note]] ![[z_media/c.png]] `![[@Videos/a.mp4]]`";
        let r = relink(text, &[done("@Videos/a.mp4", "z_media/a.mp4")]).unwrap();
        assert_eq!(r.text, "![[z_media/a.mp4]] ![[@Videos/b.mp4]] [[Note]] ![[z_media/c.png]] `![[@Videos/a.mp4]]`", "not b, not the note, not the code");
        assert_eq!(relink(text, &[done("@Videos/zzz.mp4", "z_media/z.mp4")]), None);
        assert_eq!(relink(text, &[]), None);
        // a target that is no outside link (a note named like one) is never rewritten
        assert_eq!(relink("[[Note]]", &[done("Note", "z_media/n.png")]), None);
        // non-ASCII text before a link: the char ranges are chars, not bytes
        let r = relink("Ünï ![[@Videos/ä.mp4]]", &[done("@Videos/ä.mp4", "z_media/ä.mp4")]).unwrap();
        assert_eq!(r.text, "Ünï ![[z_media/ä.mp4]]");
        assert_eq!(r.changes[0].0, 7..20);
    }
}
