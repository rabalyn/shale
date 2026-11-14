//! A path inside the vault that has been checked: relative, no `..`, no root or drive part.
//!
//! Every path a user types (rename, new note, new folder, picker, import) is turned into a
//! [`VaultPath`] first, so it can never point outside the vault (VAULT-018).

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// A relative path inside the vault. The empty path is the vault root.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct VaultPath(PathBuf);

/// Why a typed path was refused. The text is meant for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// `..` would leave the folder (or the vault).
    GoesUp,
    /// `/tmp/x`, `C:\x` or `~/x`.
    Absolute,
    /// A name had a `/` where only one name is allowed.
    NotOneName,
    Empty,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathError::GoesUp => "The name can't go above the vault (“..”).",
            PathError::Absolute => "The name must be inside the vault, not a full path.",
            PathError::NotOneName => "A name can't contain “/”. Drag it onto a folder to move it.",
            PathError::Empty => "The name can't be empty",
        })
    }
}

impl VaultPath {
    /// Check a path that is already split into components (one coming from code or a
    /// file system walk). `.` parts are dropped; `..`, a root or a drive prefix are refused.
    pub fn new(p: &Path) -> Result<VaultPath, PathError> {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::Normal(n) => out.push(n),
                Component::CurDir => {}
                Component::ParentDir => return Err(PathError::GoesUp),
                Component::RootDir | Component::Prefix(_) => return Err(PathError::Absolute),
            }
        }
        Ok(VaultPath(out))
    }

    /// Check a folder path the user typed (`Projects/ Neubau B /`). `/` and `\` both separate
    /// folders; spaces around parts and empty or `.` parts are dropped; each part is cleaned
    /// like a note name. A leading `/` is read as the vault root; `~` and `..` are refused.
    pub fn parse(typed: &str) -> Result<VaultPath, PathError> {
        let typed = typed.trim();
        if typed.starts_with('~') {
            return Err(PathError::Absolute);
        }
        let mut out = PathBuf::new();
        for part in typed.split(['/', '\\']) {
            let part = part.trim();
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                return Err(PathError::GoesUp);
            }
            if part.len() >= 2 && part.as_bytes()[1] == b':' && out.as_os_str().is_empty() {
                // `C:` – a drive, not a folder
                return Err(PathError::Absolute);
            }
            let clean = crate::markdown::sanitize_note_name(part);
            if clean.is_empty() {
                // `...` and the like: nothing left of the name
                return Err(PathError::GoesUp);
            }
            out.push(clean);
        }
        Ok(VaultPath(out))
    }

    /// Check a single file or folder name the user typed (rename). It must not contain `/` and
    /// must not be empty after cleaning.
    pub fn name(typed: &str) -> Result<String, PathError> {
        let typed = typed.trim();
        if typed.contains('/') {
            return Err(if typed.starts_with('/') { PathError::Absolute } else { PathError::NotOneName });
        }
        let clean = crate::markdown::sanitize_note_name(typed);
        if clean.is_empty() {
            return Err(if typed.trim_matches('.').is_empty() && !typed.is_empty() { PathError::GoesUp } else { PathError::Empty });
        }
        Ok(clean)
    }

    /// This path with one more checked name below it.
    pub fn join_name(&self, typed: &str) -> Result<VaultPath, PathError> {
        Ok(VaultPath(self.0.join(VaultPath::name(typed)?)))
    }

    /// This path with a typed (possibly nested) folder path below it.
    pub fn join(&self, typed: &str) -> Result<VaultPath, PathError> {
        Ok(VaultPath(self.0.join(VaultPath::parse(typed)?.0)))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for VaultPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: VAULT-018
    #[test]
    fn user_paths_stay_inside_the_vault() {
        assert_eq!(VaultPath::parse("/Projekte/ Neubau B /").unwrap().as_path(), Path::new("Projekte/Neubau B"));
        assert_eq!(VaultPath::parse("a\\b/./c").unwrap().as_path(), Path::new("a/b/c"));
        assert_eq!(VaultPath::parse("").unwrap(), VaultPath::default());
        for bad in ["..", "../x", "a/../../x", "~/x", "C:\\x", "a/.../b"] {
            assert!(VaultPath::parse(bad).is_err(), "{bad} must be refused");
        }
        assert!(VaultPath::new(Path::new("/tmp/x")).is_err());
        assert!(VaultPath::new(Path::new("a/../x")).is_err());
        assert_eq!(VaultPath::new(Path::new("./a/b")).unwrap().as_path(), Path::new("a/b"));
        // a rename name is one name
        assert_eq!(VaultPath::name(" New: name ").unwrap(), "New- name");
        assert_eq!(VaultPath::name("../../x"), Err(PathError::NotOneName));
        assert_eq!(VaultPath::name("/tmp/x"), Err(PathError::Absolute));
        assert_eq!(VaultPath::name(".."), Err(PathError::GoesUp));
        assert_eq!(VaultPath::name("   "), Err(PathError::Empty));
        let base = VaultPath::parse("Projects").unwrap();
        assert_eq!(base.join_name("Plan").unwrap().as_path(), Path::new("Projects/Plan"));
        assert!(base.join("../../etc").is_err());
        assert_eq!(base.join("a/b").unwrap().as_path(), Path::new("Projects/a/b"));
    }

    // spec: VAULT-018 (the `[!` picker: only a file's own name goes into the media folder)
    #[test]
    fn picker_names_stay_inside_the_vault() {
        for name in ["..", "photo.png", "...png", "a b #1.pdf"] {
            let rel = crate::media::media_rel_path(name, (2026, 11, 14));
            assert!(VaultPath::new(&rel).is_ok(), "{name} -> {}", rel.display());
            assert!(rel.starts_with(crate::media::MEDIA_DIR));
        }
    }
}
