//! The program search path of an app that macOS starts from Finder or the Dock (BUILD-018).
//!
//! Such an app gets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so the programs that Homebrew installs
//! (`git-lfs`, `ffmpeg`, `ffprobe`, `gpg`, `pandoc`, a newer `git`) are not found. `extended_path`
//! appends the Homebrew folders that are missing, after what is there; it never drops or reorders
//! an entry, so a `PATH` from a terminal start stays as it is.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// Homebrew's folders: Apple silicon, then Intel.
const HOMEBREW: [&str; 3] = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"];

/// `current` (the value of `PATH`, if set) with the missing Homebrew folders appended.
pub fn extended_path(current: Option<&OsStr>) -> OsString {
    let mut dirs: Vec<PathBuf> = current.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    for extra in HOMEBREW {
        if !dirs.iter().any(|d| d == std::path::Path::new(extra)) {
            dirs.push(PathBuf::from(extra));
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| current.map(OsStr::to_os_string).unwrap_or_default())
}

/// Puts the extended path into this process's environment.
#[cfg(target_os = "macos")]
pub fn apply() {
    let extended = extended_path(std::env::var_os("PATH").as_deref());
    // SAFETY: called first thing in `main`, before any thread is spawned, and the 2021 edition
    // has `set_var` safe anyway; nothing else reads or writes the environment concurrently.
    std::env::set_var("PATH", extended);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(p: &OsStr) -> Vec<String> {
        std::env::split_paths(p).map(|d| d.to_string_lossy().into_owned()).collect()
    }

    // spec: BUILD-018
    #[test]
    fn build_018_the_finder_path_gets_the_homebrew_folders_after_its_own() {
        let got = extended_path(Some(OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin")));
        assert_eq!(paths(&got), ["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"]);
    }

    // spec: BUILD-018
    #[test]
    fn build_018_a_path_from_a_terminal_keeps_its_order_and_gets_no_duplicates() {
        let got = extended_path(Some(OsStr::new("/usr/local/bin:/home/me/bin:/opt/homebrew/bin:/usr/bin")));
        assert_eq!(paths(&got), ["/usr/local/bin", "/home/me/bin", "/opt/homebrew/bin", "/usr/bin", "/opt/homebrew/sbin"]);
    }

    // spec: BUILD-018
    #[test]
    fn build_018_an_unset_path_becomes_the_homebrew_folders() {
        assert_eq!(paths(&extended_path(None)), HOMEBREW);
    }
}
