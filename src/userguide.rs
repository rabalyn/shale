//! The user guide vault on the user's computer (DEV-020): where the program finds the guide it
//! ships with, and how it puts a copy of it into a folder of its own that is opened as a vault.
//! No drawing and no window here, so the rules can be tested alone.
//!
//! The guide is `docs/userguide` of the repository. A release archive has it as `docs/userguide`
//! next to the program; a development build (`cargo run`) reads it from the source tree.

use crate::fsutil;
use std::io;
use std::path::{Path, PathBuf};

/// The note a folder must have to count as the guide.
const START_NOTE: &str = "Start here.md";

/// What of the guide is never copied: what Shale writes into a vault it opens (the same names
/// are left out of the zip of the release, `scripts/userguide-dist.sh`). Paths are relative to
/// the guide, with `/`.
pub const NOT_COPIED: [&str; 3] = [".shale/state.json", ".shale/tag-words.json", ".trash"];

/// Where the guide comes from and where its copy goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Places {
    /// the guide that ships with this program, if one was found
    pub source: Option<PathBuf>,
    /// the folder that holds one copy per version (`userguide-<version>`)
    pub root: Option<PathBuf>,
}

impl Places {
    /// The places of a program that runs on its own: the guide next to the executable or in the
    /// source tree, the copy in Shale's data folder.
    pub fn system() -> Places {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        Places { source: find(exe_dir.as_deref(), dev_source()), root: dirs::data_dir().map(|d| d.join("shale")) }
    }

    /// No guide and nowhere to copy it (tests and screenshots, which never touch the real data folder).
    pub fn none() -> Places {
        Places { source: None, root: None }
    }
}

/// The guide of the source tree, for development builds only: a release build has no business
/// reading the folder it was compiled in, which is not there on another computer.
fn dev_source() -> Option<PathBuf> {
    cfg!(debug_assertions).then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("docs").join("userguide"))
}

fn is_guide(dir: &Path) -> bool {
    dir.join(START_NOTE).is_file()
}

/// The guide to copy: `docs/userguide` next to the executable first (a release archive), else the
/// development folder given. `None` when neither holds a guide.
pub fn find(exe_dir: Option<&Path>, dev: Option<PathBuf>) -> Option<PathBuf> {
    exe_dir.map(|d| d.join("docs").join("userguide")).into_iter().chain(dev).find(|d| is_guide(d))
}

/// The folder of the copy for this version of the program.
pub fn copy_dir(root: &Path, version: &str) -> PathBuf {
    root.join(format!("userguide-{version}"))
}

/// Why there is no guide to open, in words for the person.
pub fn missing_text(version: &str) -> String {
    format!("The user guide was not found next to Shale. Download shale-userguide-{version}.zip from the release page of this version (the same page as the program), unpack it and open the folder as a vault.")
}

fn excluded(rel: &str) -> bool {
    NOT_COPIED.contains(&rel)
}

fn copy_tree(src: &Path, dest: &Path, rel: &str) -> io::Result<()> {
    std::fs::create_dir_all(dest)?;
    let mut entries: Vec<_> = std::fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if excluded(&child) {
            continue;
        }
        let kind = e.file_type()?;
        if kind.is_dir() {
            copy_tree(&e.path(), &dest.join(&name), &child)?;
        } else if kind.is_file() {
            fsutil::copy_new(&e.path(), &dest.join(&name))?;
        }
    }
    Ok(())
}

/// Make the copy of `source` for `version` in `root` and return its folder. A copy that is there
/// already is returned as it is, never touched: the person may have edited it. The copy is made
/// in a folder of its own that is renamed into place when complete, so a copy that exists is
/// always whole (like DATA-004 for files).
pub fn install(source: &Path, root: &Path, version: &str) -> io::Result<PathBuf> {
    let target = copy_dir(root, version);
    if target.exists() {
        return Ok(target);
    }
    std::fs::create_dir_all(root)?;
    let tmp = root.join(format!(".userguide-{version}.tmp{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = copy_tree(source, &tmp, "") {
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
    let Some(source) = &places.source else { return Err(missing_text(version)) };
    let Some(root) = &places.root else { return Err("There is no data folder on this computer to keep a copy of the user guide in.".into()) };
    install(source, root, version).map_err(|e| format!("The user guide could not be copied into {}: {e}", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-userguide-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A small guide with the files that must and must not be copied.
    fn guide(dir: &Path) -> PathBuf {
        let g = dir.join("guide");
        fs::create_dir_all(g.join(".shale")).unwrap();
        fs::create_dir_all(g.join(".trash")).unwrap();
        fs::create_dir_all(g.join("z_media/screenshots")).unwrap();
        fs::write(g.join(START_NOTE), "# Start\n").unwrap();
        fs::write(g.join("z_media/screenshots/a.png"), [1u8, 2, 3]).unwrap();
        fs::write(g.join(".shale/settings.json"), "{}").unwrap();
        fs::write(g.join(".shale/state.json"), "{\"tabs\":[]}").unwrap();
        fs::write(g.join(".shale/tag-words.json"), "{}").unwrap();
        fs::write(g.join(".trash/old.md"), "gone").unwrap();
        g
    }

    // spec: DEV-020
    #[test]
    fn dev_020_the_guide_is_found_next_to_the_executable_first_then_in_the_source_tree_else_nowhere() {
        let d = tmp("find");
        let exe = d.join("bin");
        let dev = guide(&d);
        fs::create_dir_all(&exe).unwrap();
        // nothing next to the executable: the development folder
        assert_eq!(find(Some(&exe), Some(dev.clone())), Some(dev.clone()));
        // a folder without the start note is no guide
        let shipped = exe.join("docs").join("userguide");
        fs::create_dir_all(&shipped).unwrap();
        assert_eq!(find(Some(&exe), Some(dev.clone())), Some(dev.clone()));
        // a release archive has its own copy, which wins
        fs::write(shipped.join(START_NOTE), "x").unwrap();
        assert_eq!(find(Some(&exe), Some(dev)), Some(shipped));
        // neither
        assert_eq!(find(None, None), None);
        assert_eq!(find(Some(&d.join("empty")), Some(d.join("also-empty"))), None);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-020
    #[test]
    fn dev_020_a_development_build_finds_the_guide_of_the_source_tree_and_it_is_a_guide() {
        let found = dev_source().expect("a debug build looks in the source tree");
        assert!(is_guide(&found), "{} has the start note", found.display());
        assert_eq!(find(None, Some(found.clone())), Some(found));
    }

    // spec: DEV-020
    #[test]
    fn dev_020_the_copy_is_made_per_version_without_what_shale_writes_itself() {
        let d = tmp("copy");
        let g = guide(&d);
        let root = d.join("data");
        let c = install(&g, &root, "1.2.3").unwrap();
        assert_eq!(c, root.join("userguide-1.2.3"));
        assert_eq!(fs::read_to_string(c.join(START_NOTE)).unwrap(), "# Start\n");
        assert_eq!(fs::read(c.join("z_media/screenshots/a.png")).unwrap(), [1, 2, 3]);
        assert!(c.join(".shale/settings.json").is_file(), "the guide's own settings (git sync off) come along");
        for n in NOT_COPIED {
            assert!(!c.join(n).exists(), "{n} is not copied");
        }
        // another version is another folder
        let c2 = install(&g, &root, "1.3.0").unwrap();
        assert_ne!(c, c2);
        assert!(c2.join(START_NOTE).is_file() && c.join(START_NOTE).is_file());
        // nothing but the two copies is left in the folder (no temporary folder)
        let mut names: Vec<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["userguide-1.2.3", "userguide-1.3.0"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-020
    #[test]
    fn dev_020_a_copy_that_exists_is_never_overwritten_and_a_failed_copy_leaves_nothing() {
        let d = tmp("keep");
        let g = guide(&d);
        let root = d.join("data");
        let c = install(&g, &root, "1.0.0").unwrap();
        // the person edits their copy, and the guide of the program changes
        fs::write(c.join(START_NOTE), "my notes\n").unwrap();
        fs::write(g.join(START_NOTE), "# Changed\n").unwrap();
        assert_eq!(install(&g, &root, "1.0.0").unwrap(), c);
        assert_eq!(fs::read_to_string(c.join(START_NOTE)).unwrap(), "my notes\n");
        // a source that is not there: no copy, no temporary folder
        let gone = d.join("no-such-guide");
        assert!(install(&gone, &root, "2.0.0").is_err());
        assert!(!copy_dir(&root, "2.0.0").exists());
        assert!(fs::read_dir(&root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with('.')));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-020
    #[test]
    fn dev_020_prepare_says_in_words_what_is_missing() {
        let d = tmp("prepare");
        let g = guide(&d);
        let none = prepare(&Places::none(), "1.2.3").unwrap_err();
        assert!(none.contains("shale-userguide-1.2.3.zip") && none.contains("release"), "{none}");
        let no_root = prepare(&Places { source: Some(g.clone()), root: None }, "1.2.3").unwrap_err();
        assert!(no_root.contains("data folder"), "{no_root}");
        let ok = prepare(&Places { source: Some(g), root: Some(d.join("data")) }, "1.2.3").unwrap();
        assert!(ok.join(START_NOTE).is_file());
        let _ = fs::remove_dir_all(&d);
    }
}
