//! The program search path of an app that macOS starts from Finder or the Dock (BUILD-018).
//!
//! Such an app gets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so the programs that Homebrew installs
//! (`git-lfs`, `ffmpeg`, `ffprobe`, `gpg`, `pandoc`, a newer `git`) are not found. `extended_path`
//! appends the Homebrew folders that are missing, after what is there; it never drops or reorders
//! an entry, so a `PATH` from a terminal start stays as it is.

use std::ffi::{OsStr, OsString};

/// Homebrew's folders: Apple silicon, then Intel.
const HOMEBREW: [&str; 3] = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"];

/// macOS PATH entries are separated by `:`. Use it explicitly rather than
/// `std::env::split_paths`/`join_paths`: those use the host's separator (`;` on
/// Windows), which would treat a `:`-separated macOS PATH as a single entry.
const PATH_SEPARATOR: char = ':';

/// `current` (the value of `PATH`, if set) with the missing Homebrew folders appended.
pub fn extended_path(current: Option<&OsStr>) -> OsString {
    let mut dirs: Vec<String> = current.map(|p| p.to_string_lossy().split(PATH_SEPARATOR).map(str::to_owned).collect()).unwrap_or_default();
    for extra in HOMEBREW {
        if !dirs.iter().any(|d| d == extra) {
            dirs.push((*extra).to_owned());
        }
    }
    dirs.join(&PATH_SEPARATOR.to_string()).into()
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
        p.to_string_lossy().split(PATH_SEPARATOR).map(str::to_owned).collect()
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

    // spec: BUILD-018
    #[test]
    fn build_018_the_colon_separator_is_used_on_every_host() {
        let got = extended_path(Some(OsStr::new("/a:/b")));
        assert_eq!(got.to_string_lossy(), "/a:/b:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin");
    }
}
