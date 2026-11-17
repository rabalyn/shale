//! Finding the external programs Shale can use (BUILD-018): ffmpeg (MEDIA-010) and pandoc
//! (IMP-036). A window program often has a shorter `PATH` than a shell (macOS apps get none of
//! the shell's), so after `PATH` the usual places are searched too.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Start `cmd`. A program file that was written a moment ago can be "text file busy" (Unix
/// `ETXTBSY`) while another thread's `fork` still holds it open for writing, which test stand-ins
/// (pretend gpg, lualatex, ffprobe) run into under load; that passes within milliseconds, so it
/// is tried again for a short while (#232, #316). Every start of an external program goes through
/// here.
pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if tries < 100 && is_busy(&e) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            r => return r,
        }
    }
}

#[cfg(unix)]
fn is_busy(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn is_busy(_: &std::io::Error) -> bool {
    false
}

/// Where programs of the user's are often installed but a window program's `PATH` lacks them.
const USUAL_DIRS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

/// The folders searched for a program: `PATH`, the usual places, then `extra` (the places of one
/// program, such as the Windows install folders of pandoc).
pub fn search_dirs(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(USUAL_DIRS.iter().map(PathBuf::from));
    dirs.extend(extra.iter().cloned());
    dirs
}

/// The first file called `name` (with `.exe` on Windows) in `dirs`.
pub fn find_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file())
}

/// The program `chosen` by the user when it is a file, else the first `name` found in `dirs`.
/// A `chosen` path that is no file finds nothing (it is not silently replaced by another program).
pub fn locate_chosen(chosen: &str, name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let chosen = chosen.trim();
    if chosen.is_empty() {
        return find_in(name, dirs);
    }
    let p = Path::new(chosen);
    p.is_file().then(|| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-program-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: IMP-036
    #[test]
    fn the_search_looks_in_path_then_the_usual_places_then_the_extra_ones() {
        let dirs = search_dirs(&[PathBuf::from("/extra/place")]);
        let at = |p: &str| dirs.iter().position(|d| d == Path::new(p));
        assert!(at("/usr/bin") < at("/extra/place") && at("/extra/place").is_some());
        assert!(at("/opt/homebrew/bin").is_some() && at("/usr/local/bin").is_some());
        if let Some(first) = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).next()) {
            assert_eq!(dirs[0], first, "PATH comes first");
        }
    }

    // spec: IMP-036
    #[test]
    fn a_program_is_found_in_a_folder_or_by_its_chosen_path() {
        let dir = temp("find");
        let name = if cfg!(windows) { "tool.exe" } else { "tool" };
        assert_eq!(find_in("tool", std::slice::from_ref(&dir)), None);
        crate::fsutil::create_new(&dir.join(name), b"x").unwrap();
        assert_eq!(find_in("tool", &[PathBuf::from("/does/not/exist"), dir.clone()]), Some(dir.join(name)));
        let dirs = [dir.clone()];
        assert_eq!(locate_chosen("", "tool", &dirs), Some(dir.join(name)), "empty: search");
        assert_eq!(locate_chosen(&dir.join(name).to_string_lossy(), "tool", &[]), Some(dir.join(name)), "a chosen file is used");
        assert_eq!(locate_chosen(&dir.join("other").to_string_lossy(), "tool", &dirs), None, "a chosen path that is no file is not replaced by the search");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
