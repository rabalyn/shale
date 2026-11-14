//! Atomic file writes (ADR-0003, DATA-004).
//!
//! A deliberately small subset of the central `fsutil` module that is being added on another
//! branch (`claude/imap-client-planning-306725`): the same function names, signatures and
//! temporary-file naming (`.shale-tmp-*`, which git sync never commits). When that branch is
//! merged, its `fsutil.rs` replaces this file as it is.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Replace `path` with `bytes` in one step: written to a temporary file in the same folder,
/// synced, then renamed over the original (keeping its permissions). A crash leaves either the
/// old or the new file, never a mix. Missing parent folders are created.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| named(dir, e))?;
    let keep = fs::metadata(path).ok().map(|m| m.permissions());
    let (mut f, tmp) = make_temp(dir).map_err(|e| named(path, e))?;
    let guard = TempGuard(Some(tmp.clone()));
    let res = (|| {
        f.write_all(bytes)?;
        if let Some(p) = keep {
            f.set_permissions(p)?;
        }
        f.sync_all()
    })();
    drop(f);
    res.map_err(|e| named(path, e))?;
    fs::rename(&tmp, path).map_err(|e| named(path, e))?;
    guard.disarm();
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

fn named(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", path.display()))
}

fn make_temp(dir: &Path) -> io::Result<(File, PathBuf)> {
    static N: AtomicU64 = AtomicU64::new(0);
    loop {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let tmp = dir.join(format!(".shale-tmp-{}-{n}-{nanos:x}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => return Ok((f, tmp)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Removes the temporary file unless the rename happened.
struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: DATA-004 (partly: the `.shale/` files)
    #[test]
    fn write_atomic_replaces_whole_and_leaves_no_temp_files() {
        let d = std::env::temp_dir().join(format!("shale-fsutil-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let p = d.join("sub").join("a.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two two");
        let names: Vec<_> = fs::read_dir(d.join("sub")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("a.json")]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
            write_atomic(&p, b"three").unwrap();
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
        }
        let _ = fs::remove_dir_all(&d);
    }
}
