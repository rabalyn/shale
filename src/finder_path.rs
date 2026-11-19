//! The program search path of an app that macOS starts from Finder or the Dock (BUILD-018).
//!
//! Such an app gets `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so the programs that Homebrew installs
//! (`git-lfs`, `ffmpeg`, `ffprobe`, `gpg`, `pandoc`, a newer `git`) are not found. `extended_path`
//! appends the Homebrew folders that are missing, after what is there; it never drops or reorders
//! an entry, so a `PATH` from a terminal start stays as it is.

use std::ffi::{OsStr, OsString};

/// Homebrew's folders: Apple silicon, then Intel.
const HOMEBREW: [&str; 3] = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"];

/// `current` (the value of `PATH`, if set) with the missing Homebrew folders appended.
///
/// A macOS `PATH` is always `:`-separated, so this splits and joins on `:` itself instead of with
/// `std::env::split_paths`, which uses `;` on Windows; the result is the same on every platform
/// (and the tests run everywhere). A `PATH` that is not valid UTF-8 is returned unchanged.
pub fn extended_path(current: Option<&OsStr>) -> OsString {
    let Some(text) = current.map_or(Some(""), OsStr::to_str) else {
        return current.map(OsStr::to_os_string).unwrap_or_default();
    };
    let mut dirs: Vec<&str> = text.split(':').filter(|d| !d.is_empty()).collect();
    for extra in HOMEBREW {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    OsString::from(dirs.join(":"))
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
        p.to_str().unwrap().split(':').map(str::to_owned).collect()
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
