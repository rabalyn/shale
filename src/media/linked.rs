//! What the outside links of a vault point to (MEDIA-022): looked up on this device for the vault
//! check (VAULT-028) and the data usage dialog (USE-026). Looking touches the disks of the outside
//! files, which can be slow, so both run it in a background job: the window only collects
//! [`Links`] from its index (no file system access) and hands them over.

use super::outside::{resolve, spot, Library, Outside, Spot};
use std::collections::HashMap;
use std::path::PathBuf;

/// The outside links of a vault as its index has them, and this device's library folders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Links {
    /// (note or drawing, link target as written): each target once for each note, in the order of the paths
    pub links: Vec<(PathBuf, String)>,
    pub libraries: Vec<Library>,
}

/// One outside link and what it points to on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Looked {
    pub note: PathBuf,
    pub target: String,
    pub spot: Spot,
    pub outside: Outside,
}

/// Look every link up (the file system is touched once for each target, however many notes use it).
pub fn look_up(links: &Links) -> Vec<Looked> {
    let mut seen: HashMap<&str, (Spot, Outside)> = HashMap::new();
    let mut out = Vec::with_capacity(links.links.len());
    for (note, target) in &links.links {
        let found = match seen.get(target.as_str()) {
            Some(f) => f.clone(),
            None => {
                let Some(s) = spot(target) else { continue };
                let f = (s.clone(), resolve(&s, &links.libraries));
                seen.insert(target, f.clone());
                f
            }
        };
        out.push(Looked { note: note.clone(), target: target.clone(), spot: found.0, outside: found.1 });
    }
    out
}

/// What the vault's outside media amount to (USE-026): files that are there, counted once for each path, with their size, and the links
/// whose file is not, counted once for each path or target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub files: u64,
    pub bytes: u64,
    pub missing: u64,
}

pub fn usage(looked: &[Looked]) -> Usage {
    let mut found: HashMap<&std::path::Path, u64> = HashMap::new();
    let mut missing: std::collections::HashSet<String> = std::collections::HashSet::new();
    for l in looked {
        match &l.outside {
            Outside::Found { abs, .. } => {
                found.entry(abs).or_insert_with(|| std::fs::metadata(abs).map(|m| m.len()).unwrap_or(0));
            }
            Outside::Missing { abs } => {
                missing.insert(abs.display().to_string());
            }
            Outside::NoLibrary(_) | Outside::LeavesLibrary { .. } => {
                missing.insert(l.target.clone());
            }
        }
    }
    Usage { files: found.len() as u64, bytes: found.values().sum(), missing: missing.len() as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    pub(crate) struct Disk {
        pub root: PathBuf,
    }

    impl Disk {
        pub fn new(tag: &str) -> Disk {
            let root = std::env::temp_dir().join(format!("shale-linked-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("Videos/2024")).unwrap();
            Disk { root }
        }

        pub fn libraries(&self) -> Vec<Library> {
            vec![Library { name: "Videos".into(), path: self.root.join("Videos") }]
        }
    }

    impl Drop for Disk {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn links(disk: &Disk, targets: &[(&str, &str)]) -> Links {
        Links { links: targets.iter().map(|(n, t)| (PathBuf::from(n), t.to_string())).collect(), libraries: disk.libraries() }
    }

    // spec: MEDIA-022
    #[test]
    fn every_link_is_looked_up_on_this_device_and_a_target_is_looked_at_once() {
        let d = Disk::new("look");
        fs::write(d.root.join("Videos/2024/a.mp4"), b"12345").unwrap();
        let l = links(&d, &[("A.md", "@Videos/2024/a.mp4"), ("B.md", "@Videos/2024/a.mp4"), ("A.md", "@Videos/gone.mp3"), ("A.md", "@Nowhere/x.png"), ("A.md", "@Videos/../y.png")]);
        let looked = look_up(&l);
        assert_eq!(looked.len(), 5);
        assert!(matches!(&looked[0].outside, Outside::Found { abs, .. } if abs.ends_with("2024/a.mp4")));
        assert_eq!((looked[1].note.as_path(), &looked[1].outside), (std::path::Path::new("B.md"), &looked[0].outside));
        assert!(matches!(&looked[2].outside, Outside::Missing { abs } if abs.ends_with("gone.mp3")));
        assert_eq!(looked[3].outside, Outside::NoLibrary("Nowhere".into()));
        assert_eq!(looked[4].outside, Outside::LeavesLibrary { name: "Videos".into() });
        assert!(matches!(&looked[0].spot, Spot::Library { name, .. } if name == "Videos"));
        // a target that is no outside link is left out
        assert!(look_up(&links(&d, &[("A.md", "Note")])).is_empty());
        assert!(look_up(&Links::default()).is_empty());
    }

    // spec: MEDIA-022, USE-026
    #[test]
    fn the_usage_counts_each_file_once_by_path_with_its_size_and_the_missing_apart() {
        let d = Disk::new("usage");
        fs::write(d.root.join("Videos/2024/a.mp4"), vec![0u8; 1000]).unwrap();
        fs::write(d.root.join("Videos/b.png"), vec![0u8; 24]).unwrap();
        let by_address = crate::media::testdata::file_address(&d.root.join("Videos/2024/a.mp4"));
        let l = links(
            &d,
            &[
                ("A.md", "@Videos/2024/a.mp4"),
                ("B.md", "@Videos/2024/a.mp4"),
                ("C.md", by_address.as_str()),
                ("A.md", "@Videos/b.png"),
                ("A.md", "@Videos/gone.mp3"),
                ("B.md", "@Videos/gone.mp3"),
                ("A.md", "@Nowhere/x.png"),
                ("A.md", "@Videos/../y.png"),
            ],
        );
        let u = usage(&look_up(&l));
        assert_eq!(u, Usage { files: 2, bytes: 1024, missing: 3 }, "a.mp4 and b.png; gone.mp3 once, the library not set up, the path out of the library");
        assert_eq!(usage(&[]), Usage::default());
    }
}
