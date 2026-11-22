//! The links to media outside the vault in the vault check (MEDIA-022, VAULT-028): which links of
//! which notes name a file that is not there on this device, a library that is not set up or whose
//! folder is not there, and which are file links (`file://…`), which work on this computer only.
//!
//! [`check_outside`] looks the files up, so the check runs it in the same background job as the
//! check of the names; the window gives it [`Links`] from its index.

use crate::media::linked::{look_up, Links, Looked};
use crate::media::outside::{Outside, Spot};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What is the matter with one outside link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// the library is not set up on this device
    NoLibrary(String),
    /// the path leaves the library folder
    LeavesLibrary(String),
    /// the file is not there
    Missing(PathBuf),
    /// the file is there, but a file link means this computer only: it does not work on another device
    FileLink,
}

impl Problem {
    /// In words, for the list.
    pub fn text(&self) -> String {
        match self {
            Problem::NoLibrary(name) => format!("library “{name}” is not set up on this device"),
            Problem::LeavesLibrary(name) => format!("leaves the library folder “{name}”"),
            Problem::Missing(abs) => format!("not found: {}", abs.display()),
            Problem::FileLink => "a file link: it works on this computer only; a library link would work on every device".to_string(),
        }
    }
}

/// One link that needs a look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// the target as written in the note
    pub target: String,
    pub problem: Problem,
}

/// The links of one note that need a look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteLinks {
    pub note: PathBuf,
    pub entries: Vec<Entry>,
}

/// What is the matter with a library that links name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderState {
    NotSetUp,
    NotThere(PathBuf),
}

/// A library that links use and that cannot be used on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryUse {
    pub name: String,
    /// how many links name it
    pub links: usize,
    pub state: FolderState,
}

/// The outside links of the vault, and what needs a look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutsideCheck {
    /// all outside links (a target once for each note)
    pub links: usize,
    /// the notes with links that need a look, in the order of their paths
    pub notes: Vec<NoteLinks>,
    /// the libraries that are not set up or whose folder is not there, by name
    pub libraries: Vec<LibraryUse>,
}

impl OutsideCheck {
    /// How many links need a look.
    pub fn problems(&self) -> usize {
        self.notes.iter().map(|n| n.entries.len()).sum()
    }
}

fn problem(l: &Looked) -> Option<Problem> {
    match &l.outside {
        Outside::Found { .. } => matches!(l.spot, Spot::File(_)).then_some(Problem::FileLink),
        Outside::Missing { abs } => Some(Problem::Missing(abs.clone())),
        Outside::NoLibrary(name) => Some(Problem::NoLibrary(name.clone())),
        Outside::LeavesLibrary { name } => Some(Problem::LeavesLibrary(name.clone())),
    }
}

/// Look the outside links up and sort out what needs a look.
pub fn check_outside(links: &Links) -> OutsideCheck {
    let looked = look_up(links);
    let mut notes: Vec<NoteLinks> = Vec::new();
    // the libraries the links name, by name in any case: the name as first written and how many links
    let mut used: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for l in &looked {
        if let Spot::Library { name, .. } = &l.spot {
            used.entry(name.to_lowercase()).or_insert_with(|| (name.clone(), 0)).1 += 1;
        }
        let Some(problem) = problem(l) else { continue };
        let entry = Entry { target: l.target.clone(), problem };
        match notes.last_mut().filter(|n| n.note == l.note) {
            Some(n) => n.entries.push(entry),
            None => notes.push(NoteLinks { note: l.note.clone(), entries: vec![entry] }),
        }
    }
    let libraries = used
        .into_iter()
        .filter_map(|(key, (name, count))| {
            let set_up = links.libraries.iter().find(|lib| lib.name.to_lowercase() == key && !lib.path.as_os_str().is_empty());
            let state = match set_up {
                None => FolderState::NotSetUp,
                Some(lib) if !lib.path.is_dir() => FolderState::NotThere(lib.path.clone()),
                Some(_) => return None,
            };
            Some(LibraryUse { name, links: count, state })
        })
        .collect();
    OutsideCheck { links: looked.len(), notes, libraries }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::outside::Library;
    use std::fs;

    struct Disk {
        root: PathBuf,
    }

    impl Disk {
        fn new(tag: &str) -> Disk {
            let root = std::env::temp_dir().join(format!("shale-vcheck-outside-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("Videos")).unwrap();
            fs::create_dir_all(root.join("Music")).unwrap();
            fs::write(root.join("Videos/a.mp4"), b"a").unwrap();
            fs::write(root.join("loose.png"), b"p").unwrap();
            Disk { root }
        }
    }

    impl Drop for Disk {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn links(list: &[(&str, &str)], libraries: Vec<Library>) -> Links {
        Links { links: list.iter().map(|(n, t)| (PathBuf::from(n), t.to_string())).collect(), libraries }
    }

    // spec: MEDIA-022, VAULT-028
    #[test]
    fn the_check_lists_by_note_what_is_missing_what_is_not_set_up_and_which_links_are_file_links() {
        let d = Disk::new("list");
        let file_link = crate::media::testdata::file_address(&d.root.join("loose.png"));
        let gone_link = crate::media::testdata::file_address(&d.root.join("gone.png"));
        let libs = vec![Library { name: "Videos".into(), path: d.root.join("Videos") }, Library { name: "Music".into(), path: d.root.join("no-such-folder") }];
        let l = links(
            &[
                ("A.md", "@Videos/a.mp4"),
                ("A.md", "@videos/gone.mp4"),
                ("A.md", "@Nowhere/b.mp3"),
                ("A.md", file_link.as_str()),
                ("B.md", "@Music/c.mp3"),
                ("B.md", "@Videos/../x.mp4"),
                ("B.md", gone_link.as_str()),
                ("C.md", "@Videos/a.mp4"),
            ],
            libs,
        );
        let c = check_outside(&l);
        assert_eq!(c.links, 8);
        assert_eq!(c.notes.iter().map(|n| n.note.to_str().unwrap()).collect::<Vec<_>>(), ["A.md", "B.md"], "C.md has nothing to look at: its library link works");
        let a: Vec<_> = c.notes[0].entries.iter().map(|e| (e.target.as_str(), &e.problem)).collect();
        assert_eq!(a[0], ("@videos/gone.mp4", &Problem::Missing(d.root.join("Videos/gone.mp4"))), "the library name matches in any case");
        assert_eq!(a[1], ("@Nowhere/b.mp3", &Problem::NoLibrary("Nowhere".into())));
        assert_eq!(a[2], (file_link.as_str(), &Problem::FileLink), "a file link that works is still listed: it works here only");
        assert_eq!(a.len(), 3, "the library link that works is not listed");
        let b: Vec<_> = c.notes[1].entries.iter().map(|e| &e.problem).collect();
        assert_eq!(b[0], &Problem::Missing(d.root.join("no-such-folder/c.mp3")));
        assert_eq!(b[1], &Problem::LeavesLibrary("Videos".into()));
        assert_eq!(b[2], &Problem::Missing(d.root.join("gone.png")));
        assert_eq!(c.problems(), 6);
        // the libraries that cannot be used: one not set up, one whose folder is not there; "Videos" is fine
        assert_eq!(c.libraries, vec![LibraryUse { name: "Music".into(), links: 1, state: FolderState::NotThere(d.root.join("no-such-folder")) }, LibraryUse { name: "Nowhere".into(), links: 1, state: FolderState::NotSetUp }]);
        // a library with an empty folder is not set up
        let c = check_outside(&links(&[("A.md", "@Videos/a.mp4")], vec![Library { name: "Videos".into(), path: PathBuf::new() }]));
        assert_eq!(c.libraries, vec![LibraryUse { name: "Videos".into(), links: 1, state: FolderState::NotSetUp }]);
        // nothing linked: nothing to say
        assert_eq!(check_outside(&Links::default()), OutsideCheck::default());
    }

    // spec: MEDIA-022
    #[test]
    fn a_problem_says_what_is_wrong_in_words() {
        assert_eq!(Problem::NoLibrary("Videos".into()).text(), "library “Videos” is not set up on this device");
        assert_eq!(Problem::LeavesLibrary("Videos".into()).text(), "leaves the library folder “Videos”");
        assert!(Problem::Missing(PathBuf::from("/x/a.mp4")).text().starts_with("not found: /x/a.mp4"));
        assert!(Problem::FileLink.text().contains("works on this computer only"));
    }
}
