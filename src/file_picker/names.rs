//! The name of a new folder made in Shale's own picker (PICK-008, #688): what is checked before
//! anything is made, and what happens when the name is taken.

use crate::fsutil::{portable_problems, TYPED_NAME_MAX};
use std::path::{Path, PathBuf};

/// What a typed folder name may be: not empty, one name (no `/` or `\`), not `.` or `..`, and
/// working on Windows too (none of its forbidden characters, no trailing dot, no device name such
/// as `CON` or `NUL`, not too long). Outer spaces are not part of the name. The reason is a sentence.
pub fn check_folder_name(typed: &str) -> Result<String, String> {
    let name = typed.trim();
    if name.is_empty() {
        return Err("Type a name for the new folder.".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("A folder name cannot contain / or \\: it is one name, made inside the shown folder.".into());
    }
    if name == "." || name == ".." {
        return Err(format!("“{name}” is not a name for a folder."));
    }
    let problems = portable_problems(name, TYPED_NAME_MAX);
    if let Some(first) = problems.first() {
        return Err(format!("“{name}” cannot be used: {first}."));
    }
    Ok(name.to_string())
}

/// What making the folder `name` inside `dir` came to.
#[derive(Debug, PartialEq)]
pub enum Made {
    /// the folder was made
    New(PathBuf),
    /// a folder of this name is there already: the picker offers to use it
    Exists(PathBuf),
}

/// Make the folder `typed` inside `dir` (PICK-008). A name that is refused, a file of that name and
/// a folder that cannot be made give their reason; a folder that is there already is not an error
/// but [`Made::Exists`].
pub fn make_folder(dir: &Path, typed: &str) -> Result<Made, String> {
    let name = check_folder_name(typed)?;
    let path = dir.join(&name);
    if path.is_dir() {
        return Ok(Made::Exists(path));
    }
    if path.exists() {
        return Err(format!("“{name}” is a file in this folder, so it cannot be a folder."));
    }
    std::fs::create_dir(&path).map_err(|e| format!("Could not make the folder “{name}”: {e}"))?;
    Ok(Made::New(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-pick-names-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: PICK-008
    #[test]
    fn pick_008_a_good_name_is_kept_without_its_outer_spaces() {
        assert_eq!(check_folder_name("  Neue Notizen "), Ok("Neue Notizen".to_string()));
        assert_eq!(check_folder_name("Café 2026"), Ok("Café 2026".to_string()));
        assert_eq!(check_folder_name(".hidden"), Ok(".hidden".to_string()));
    }

    // spec: PICK-008
    #[test]
    fn pick_008_empty_names_separators_dots_and_windows_names_are_refused_with_a_reason() {
        for (typed, part) in [
            ("", "Type a name"),
            ("   ", "Type a name"),
            ("a/b", "cannot contain"),
            ("a\\b", "cannot contain"),
            (".", "not a name for a folder"),
            ("..", "not a name for a folder"),
            ("a:b", "not allowed on Windows"),
            ("what?", "not allowed on Windows"),
            ("name.", "end in a dot"),
            ("CON", "reserved name"),
            ("nul", "reserved name"),
            ("Com1", "reserved name"),
            ("tab\tname", "control characters"),
        ] {
            let e = check_folder_name(typed).expect_err(typed);
            assert!(e.contains(part), "{typed:?}: {e}");
        }
        assert!(check_folder_name(&"x".repeat(300)).unwrap_err().contains("longer than"));
    }

    // spec: PICK-008
    #[test]
    fn pick_008_a_new_folder_is_made_inside_the_shown_one_and_an_existing_one_is_reported() {
        let d = tmp("make");
        assert_eq!(make_folder(&d, " Fresh ").unwrap(), Made::New(d.join("Fresh")));
        assert!(d.join("Fresh").is_dir());
        assert_eq!(make_folder(&d, "Fresh").unwrap(), Made::Exists(d.join("Fresh")), "nothing is made twice");
        std::fs::write(d.join("note.md"), "x").unwrap();
        assert!(make_folder(&d, "note.md").unwrap_err().contains("is a file"));
        assert!(make_folder(&d, "a/b").is_err() && !d.join("a").exists(), "a refused name makes nothing");
        // the shown folder is gone: the reason of the system is shown
        let e = make_folder(&d.join("gone"), "x").unwrap_err();
        assert!(e.starts_with("Could not make the folder “x”: "), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
