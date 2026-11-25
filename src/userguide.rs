//! The user guide vault that is built into the program (DEV-020): `docs/userguide` is embedded
//! in the executable at compile time (`build.rs`), and this puts a copy of it into a folder of
//! Shale's own that is opened as a vault. No drawing and no window here, so the rules can be
//! tested alone. It works in every build on every platform: nothing is looked up next to the
//! program or in the source tree.

use crate::fsutil;
use std::io;
use std::path::{Path, PathBuf};

/// The files of a guide: path relative to the guide (with `/`) and content.
pub type Files = &'static [(&'static str, &'static [u8])];

/// The guide of this program, `docs/userguide` without what Shale writes into a vault. A `static`,
/// not a `const`: a const is copied into every codegen unit that uses it, which put the whole
/// guide into the executable twice (BUILD-044).
pub static EMBEDDED: Files = include!(concat!(env!("OUT_DIR"), "/userguide_files.rs"));

/// What of the guide is never bundled: what Shale writes into a vault it opens (`build.rs` leaves
/// the same names out of the table, and `scripts/userguide-dist.sh` out of the zip of the
/// release; tests check the three agree). Paths are relative to the guide, with `/`.
#[cfg(test)]
pub const NOT_COPIED: [&str; 3] = [".shale/state.json", ".shale/tag-words.json", ".trash"];

/// Which guide to unpack and where its copy goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Places {
    /// the files of the guide
    pub files: Files,
    /// the folder that holds one copy per version (`userguide-<version>`)
    pub root: Option<PathBuf>,
}

impl Places {
    /// The places of a program that runs on its own: the embedded guide, the copy in Shale's data folder.
    pub fn system() -> Places {
        Places { files: EMBEDDED, root: crate::userdirs::data_dir().map(|d| d.join("shale")) }
    }

    /// Nowhere to copy the guide to (tests and screenshots, which never touch the real data folder).
    pub fn none() -> Places {
        Places { files: EMBEDDED, root: None }
    }
}

/// The folder of the copy for this version of the program.
pub fn copy_dir(root: &Path, version: &str) -> PathBuf {
    root.join(format!("userguide-{version}"))
}

/// Write every file of `files` below `dest`, which is made.
fn unpack(files: Files, dest: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for (rel, bytes) in files {
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        fsutil::create_new(&path, bytes)?;
    }
    Ok(())
}

/// Unpack `files` as the copy for `version` in `root` and return its folder. A copy that is there
/// already is returned as it is, never touched: the person may have edited it. The copy is made
/// in a folder of its own that is renamed into place when complete, so a copy that exists is
/// always whole (like DATA-004 for files).
pub fn install(files: Files, root: &Path, version: &str) -> io::Result<PathBuf> {
    let target = copy_dir(root, version);
    if target.exists() {
        return Ok(target);
    }
    std::fs::create_dir_all(root)?;
    let tmp = root.join(format!(".userguide-{version}.tmp{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = unpack(files, &tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let renamed = fsutil::rename_noreplace(&tmp, &target);
    if renamed.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    match renamed {
        Ok(()) => Ok(target),
        // another Shale was faster: its copy stays
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(target),
        Err(e) => Err(e),
    }
}

/// The folder to open as a vault: the copy for `version`, made if it is not there yet. The
/// error is a sentence for the person.
pub fn prepare(places: &Places, version: &str) -> Result<PathBuf, String> {
    let Some(root) = &places.root else { return Err("There is no data folder on this computer to keep a copy of the user guide in.".into()) };
    install(places.files, root, version).map_err(|e| format!("The user guide could not be unpacked into {}: {e}", root.display()))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;

    /// A small guide for tests that must not unpack the whole one.
    pub const SAMPLE: Files = &[("Start here.md", b"# Start\n\n[[Other]]\n"), ("Other.md", b"other\n"), ("z_media/screenshots/a.png", &[1, 2, 3]), (".shale/settings.json", b"{}")];

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-userguide-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: DEV-020
    #[test]
    fn dev_020_the_guide_is_embedded_with_its_start_note_and_without_what_shale_writes_itself() {
        let names: Vec<&str> = EMBEDDED.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"Start here.md"), "the start note is embedded");
        assert!(names.contains(&".shale/settings.json"), "the guide's own settings (git sync off) are embedded");
        assert!(names.iter().any(|n| n.starts_with("z_media/screenshots/")), "the pictures are embedded");
        for n in NOT_COPIED {
            assert!(!names.iter().any(|f| *f == n || f.starts_with(&format!("{n}/"))), "{n} is not embedded");
        }
        let start = EMBEDDED.iter().find(|(n, _)| *n == "Start here.md").unwrap().1;
        assert_eq!(start, fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/userguide/Start here.md")).unwrap(), "the embedded note is the one of the source tree");
    }

    // spec: DEV-020
    #[test]
    fn dev_020_unpacking_writes_every_embedded_file_per_version() {
        let d = tmp("unpack");
        let root = d.join("data");
        let c = install(EMBEDDED, &root, "1.2.3").unwrap();
        assert_eq!(c, root.join("userguide-1.2.3"));
        for (rel, bytes) in EMBEDDED {
            assert_eq!(&fs::read(c.join(rel)).unwrap(), bytes, "{rel}");
        }
        // another version is another folder, and no temporary folder is left
        let c2 = install(SAMPLE, &root, "1.3.0").unwrap();
        assert_ne!(c, c2);
        assert_eq!(fs::read(c2.join("z_media/screenshots/a.png")).unwrap(), [1, 2, 3]);
        let mut names: Vec<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["userguide-1.2.3", "userguide-1.3.0"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-020
    #[test]
    fn dev_020_a_copy_that_exists_is_never_overwritten_and_a_failed_unpack_leaves_nothing() {
        let d = tmp("keep");
        let root = d.join("data");
        let c = install(SAMPLE, &root, "1.0.0").unwrap();
        fs::write(c.join("Start here.md"), "my notes\n").unwrap();
        assert_eq!(install(SAMPLE, &root, "1.0.0").unwrap(), c);
        assert_eq!(fs::read_to_string(c.join("Start here.md")).unwrap(), "my notes\n");
        // the same file twice cannot be written: no copy, no temporary folder
        let twice: Files = &[("a.md", b"1"), ("a.md", b"2")];
        assert!(install(twice, &root, "2.0.0").is_err());
        assert!(!copy_dir(&root, "2.0.0").exists());
        assert!(fs::read_dir(&root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with('.')));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-020
    #[test]
    fn dev_020_prepare_says_in_words_what_is_missing() {
        let d = tmp("prepare");
        let no_root = prepare(&Places::none(), "1.2.3").unwrap_err();
        assert!(no_root.contains("data folder"), "{no_root}");
        let ok = prepare(&Places { files: SAMPLE, root: Some(d.join("data")) }, "1.2.3").unwrap();
        assert!(ok.join("Start here.md").is_file());
        // a root that is a file cannot hold the copy
        fs::write(d.join("file"), "x").unwrap();
        let bad = prepare(&Places { files: SAMPLE, root: Some(d.join("file")) }, "1.2.3").unwrap_err();
        assert!(bad.contains("could not be unpacked"), "{bad}");
        let _ = fs::remove_dir_all(&d);
    }
}
