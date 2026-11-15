//! The one place where user data is read from and written to disk.
//!
//! "We never lose or corrupt data": a crash, a full disk or a second writer must never leave a
//! half-written file. Every save in Shale therefore goes through this module, so the safe
//! behaviour is written, and tested, exactly once. A test (`no_unsafe_file_calls_outside_fsutil`)
//! fails if any other source file calls `fs::write`, `File::create`, `File::open`, `OpenOptions`,
//! `fs::copy`, `fs::read`, `fs::read_to_string` or `fs::rename`.
//!
//! - [`write_atomic`] replaces a file in one step: the new content is written to a temporary
//!   file in the same folder, synced to disk and renamed over the original. Readers see the old
//!   or the new file, never a mix. On any error the original is untouched and the temporary
//!   file is removed.
//! - [`create_new`] and [`copy_new`] create a file but never overwrite an existing one.
//! - [`rename_noreplace`] moves a file or folder but never replaces the target.
//! - [`read`] and [`read_to_string`] return errors that name the file; nothing is silently empty.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Permission bits of a new file before the umask is applied (what `fs::write` uses).
#[cfg(unix)]
const DEFAULT_MODE: u32 = 0o666;
/// Permission bits of a private file (settings, secrets): the owner only.
#[cfg(unix)]
const PRIVATE_MODE: u32 = 0o600;

/// How new files get their permissions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Perms {
    /// A file that exists keeps its permissions; a new one gets the usual default.
    Normal,
    /// The file is owner-only, whatever it was before.
    Private,
}

/// Replace `path` with `bytes` in one step (see the module comment). Makes no folders.
/// An existing file keeps its permissions; a new one gets the default for new files.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, Perms::Normal, |f| f.write_all(bytes))
}

/// Like [`write_atomic`], for files only the owner may read (settings, tokens): the file is
/// created with mode 0600, so it is never readable by others, not even for a moment.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, Perms::Private, |f| f.write_all(bytes))
}

/// Create `path` with `bytes`; fails with `AlreadyExists` if something is there already.
/// The file appears complete or not at all, and an existing file is never clobbered.
pub fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    create_new_with(path, |f| f.write_all(bytes))
}

/// Copy the file `src` to `dest`, which must not exist (`AlreadyExists` otherwise). Atomic like
/// [`create_new`]; the copy keeps the permissions of the original.
pub fn copy_new(src: &Path, dest: &Path) -> io::Result<()> {
    let mut from = File::open(src).map_err(|e| named(src, e))?;
    create_new_with(dest, |f| io::copy(&mut from, f).map(|_| ()))?;
    #[cfg(unix)]
    {
        // best effort: the content is safely in place, a failure here is not worth an error
        let _ = from.metadata().and_then(|m| fs::set_permissions(dest, m.permissions()));
    }
    Ok(())
}

/// Open a file for reading. On Windows a file that is being replaced at that moment (by an
/// atomic save of another program, or of Shale) is "access denied" for a few milliseconds, so
/// that error is tried again for a moment before it counts (DATA-013).
fn open_retrying(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        let mut tries = 0;
        loop {
            match File::open(path) {
                Err(e) if tries < 40 && e.kind() == io::ErrorKind::PermissionDenied => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                r => return r,
            }
        }
    }
    #[cfg(not(windows))]
    File::open(path)
}

/// Read a whole file. The error says which file could not be read.
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref();
    let mut buf = Vec::new();
    open_retrying(path).and_then(|mut f| f.read_to_end(&mut buf)).map_err(|e| named(path, e))?;
    Ok(buf)
}

/// Read a whole file as UTF-8 text. The error says which file, and a file that is not valid
/// UTF-8 is an `InvalidData` error, never a lossy or empty string.
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|_| named(path, io::Error::new(io::ErrorKind::InvalidData, "not valid UTF-8 text")))
}

/// Open a file for reading (for files too big or too odd to read whole, such as mailboxes).
/// The error says which file could not be opened. (Only the Thunderbird lookup of dropped
/// messages uses it, and that is Linux only.)
#[cfg(target_os = "linux")]
pub fn open_read(path: impl AsRef<Path>) -> io::Result<File> {
    let path = path.as_ref();
    open_retrying(path).map_err(|e| named(path, e))
}

/// Flush a file this program did not write itself (a copy another library made, such as a
/// database backup) to disk. The error says which file.
pub fn sync_file(path: &Path) -> io::Result<()> {
    // Windows' FlushFileBuffers wants write access to the file; elsewhere a read-only open is enough
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    o.write(true);
    o.open(path).and_then(|f| f.sync_all()).map_err(|e| named(path, e))
}

/// Windows: what tells one file from another even when its name and times are the same: the volume
/// and the file index (the counterpart of the inode, DATA-002). A file made by an atomic save has
/// a new one. None if it cannot be read.
#[cfg(windows)]
pub fn file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let f = File::open(path).ok()?;
    // SAFETY: the handle is open for as long as `f` lives and the struct is filled by the call.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    Some((info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)))
}

/// Make a folder (and its parents) that only the owner can enter (mode 0700). A folder that
/// exists gets its permissions tightened.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Make one new folder that only the owner can enter. A path that already exists (a folder or a
/// link someone else made) is an error: it is never adopted.
pub fn create_private_dir_new(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    let b = {
        use std::os::unix::fs::DirBuilderExt;
        let mut b = fs::DirBuilder::new();
        b.mode(0o700);
        b
    };
    // Windows: the new folder inherits the access list of its place (DATA-014)
    #[cfg(not(unix))]
    let b = fs::DirBuilder::new();
    b.create(path).map_err(|e| named(path, e))
}

/// A folder `<prefix>-<user id>` for temporary files that must stay private: in the user's runtime
/// folder when there is one (already private), else in the system temp folder, where an existing
/// path of that name that is not a folder of the current user is refused.
pub fn private_temp_root(prefix: &str) -> io::Result<PathBuf> {
    #[cfg(unix)]
    let who = unsafe { libc::geteuid() }.to_string();
    // Windows: the system temp folder is the user's own (`%TEMP%` inside the profile, DATA-014)
    #[cfg(not(unix))]
    let who = String::from("user");
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("{prefix}-{who}"));
    if let Ok(m) = fs::symlink_metadata(&dir) {
        #[cfg(unix)]
        let ours = {
            use std::os::unix::fs::MetadataExt;
            m.is_dir() && m.uid() == unsafe { libc::geteuid() }
        };
        // no owner id to compare here: a plain folder (not a link) in the user's own temp folder
        #[cfg(not(unix))]
        let ours = m.is_dir() && !m.file_type().is_symlink();
        if !ours {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the temp folder is not ours"));
        }
    }
    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// Open (creating it owner-only) a lock file and take an exclusive advisory lock on it without
/// waiting. `Ok(Some(file))`: the lock is held as long as the file value lives; the operating
/// system releases it when the process ends, also after a crash. `Ok(None)`: another process
/// (or another open of the file) holds it. Any other problem is an error.
/// (`flock` on Linux and macOS, `LockFileEx` on Windows, through the standard library.)
pub fn try_lock_file(path: &Path) -> io::Result<Option<File>> {
    if let Some(dir) = path.parent() {
        ensure_private_dir(dir)?;
    }
    let mut o = OpenOptions::new();
    o.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(PRIVATE_MODE);
    }
    let f = o.open(path).map_err(|e| named(path, e))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(named(path, e)),
    }
}

/// Open an owner-only file for a debug log (not user data): started empty, or with `append`
/// added to. Created with mode 0600.
pub fn open_private_log(path: &Path, append: bool) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        ensure_private_dir(dir)?;
    }
    let mut o = OpenOptions::new();
    o.create(true);
    if append {
        o.append(true);
    } else {
        o.write(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        o.mode(PRIVATE_MODE);
        let f = o.open(path)?;
        // an older file may have been created with wider permissions
        f.set_permissions(fs::Permissions::from_mode(PRIVATE_MODE))?;
        Ok(f)
    }
    #[cfg(not(unix))]
    o.open(path)
}

/// Move a file or folder to `to`, which must not exist (`AlreadyExists` otherwise). Unlike a
/// plain rename this can never replace something at the target, even if it appeared a moment
/// ago. Where the file system can't promise that, it falls back to checking first.
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    match renameat2_noreplace(from, to) {
        Some(r) => r,
        None => {
            // no atomic way here: a hard link fails if the target exists (files only)
            #[cfg(unix)]
            if fs::symlink_metadata(from).map(|m| m.is_file()).unwrap_or(false) {
                match fs::hard_link(from, to) {
                    Ok(()) => return fs::remove_file(from),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
                    Err(_) => {}
                }
            }
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the target exists"));
            }
            fs::rename(from, to)
        }
    }
}

/// `renameat2(RENAME_NOREPLACE)` on Linux. `None` when it is not available here (old kernels,
/// file systems without support).
#[cfg(target_os = "linux")]
fn renameat2_noreplace(from: &Path, to: &Path) -> Option<io::Result<()>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let a = CString::new(from.as_os_str().as_bytes()).ok()?;
    let b = CString::new(to.as_os_str().as_bytes()).ok()?;
    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call.
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_NOREPLACE) };
    if r == 0 {
        return Some(Ok(()));
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::ENOTSUP) => None,
        _ => Some(Err(e)),
    }
}

/// `renamex_np(RENAME_EXCL)` on macOS. `None` when the file system does not support it.
#[cfg(target_os = "macos")]
fn renameat2_noreplace(from: &Path, to: &Path) -> Option<io::Result<()>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let a = CString::new(from.as_os_str().as_bytes()).ok()?;
    let b = CString::new(to.as_os_str().as_bytes()).ok()?;
    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call.
    let r = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) };
    if r == 0 {
        return Some(Ok(()));
    }
    let e = io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ENOTSUP) | Some(libc::EINVAL) => None,
        _ => Some(Err(e)),
    }
}

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` on Windows: it fails if the target exists
/// (`std::fs::rename` would replace it).
#[cfg(windows)]
fn renameat2_noreplace(from: &Path, to: &Path) -> Option<io::Result<()>> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(std::iter::once(0)).collect() };
    let (a, b) = (wide(from), wide(to));
    // SAFETY: both buffers are NUL-terminated UTF-16 strings that outlive the call.
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::MoveFileExW(a.as_ptr(), b.as_ptr(), 0) };
    if ok != 0 {
        return Some(Ok(()));
    }
    let e = io::Error::last_os_error();
    // ERROR_ALREADY_EXISTS (183) and ERROR_FILE_EXISTS (80) say AlreadyExists; a folder at the
    // target may also come back as access denied, so look
    if matches!(e.raw_os_error(), Some(80) | Some(183)) || (e.kind() == io::ErrorKind::PermissionDenied && fs::symlink_metadata(to).is_ok()) {
        return Some(Err(io::Error::new(io::ErrorKind::AlreadyExists, "the target exists")));
    }
    Some(Err(e))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn renameat2_noreplace(_: &Path, _: &Path) -> Option<io::Result<()>> {
    None
}

// ---- internals ----

/// An error that says which file it is about.
fn named(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", path.display()))
}

/// Counter that makes temporary names unique inside this process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Removes the temporary file when dropped, unless it was renamed into place.
struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn disarm(&mut self) {
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

/// The file to really write: `path` itself, or what it points to if it is a symbolic link (a
/// rename would otherwise replace the link with a plain file).
fn resolve(path: &Path) -> io::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(path).map_err(|e| named(path, e)),
        _ => Ok(path.to_path_buf()),
    }
}

/// A new, empty, uniquely named temporary file in the folder of `target` (created with
/// `O_EXCL`, so it can never be an existing file or somebody's link).
fn make_temp(target: &Path, perms: Perms) -> io::Result<(File, PathBuf)> {
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    for _ in 0..100 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let tmp = dir.join(format!(".shale-tmp-{}-{n}-{nanos:x}", std::process::id()));
        let mut o = OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(if perms == Perms::Private { PRIVATE_MODE } else { DEFAULT_MODE });
        }
        #[cfg(not(unix))]
        let _ = perms;
        match o.open(&tmp) {
            Ok(f) => return Ok((f, tmp)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(named(target, e)),
        }
    }
    Err(named(target, io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name")))
}

/// Write a temporary file with `fill`, and make it durable (data synced) with the right permissions.
fn write_temp(target: &Path, perms: Perms, keep: Option<&fs::Permissions>, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<(TempGuard, PathBuf)> {
    let (mut f, tmp) = make_temp(target, perms)?;
    let guard = TempGuard(Some(tmp.clone()));
    fill(&mut f).map_err(|e| named(target, e))?;
    #[cfg(unix)]
    {
        if perms == Perms::Private {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(PRIVATE_MODE)).map_err(|e| named(target, e))?;
        } else if let Some(p) = keep {
            f.set_permissions(p.clone()).map_err(|e| named(target, e))?;
        }
    }
    #[cfg(not(unix))]
    let _ = keep;
    f.sync_all().map_err(|e| named(target, e))?;
    Ok((guard, tmp))
}

/// Make the rename of a file in `dir` durable. Best effort: the content is already in place and
/// some file systems can't sync a folder, so a failure here is not reported as a failed save.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    {
        let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Rename `from` over `to` (the replacing rename of an atomic save, DATA-013). On Windows the
/// target can be held open for a moment by a program that reads it (a virus scanner, the search
/// indexer, another reader of Shale): the rename then fails with "access denied" or "sharing
/// violation" and works a moment later, so it is tried again for about a second.
fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let mut tries = 0;
        loop {
            match fs::rename(from, to) {
                Err(e) if tries < 40 && (e.kind() == io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32)) => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                r => return r,
            }
        }
    }
    #[cfg(not(windows))]
    fs::rename(from, to)
}

fn write_atomic_with(path: &Path, perms: Perms, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let target = resolve(path)?;
    let existing = match fs::metadata(&target) {
        Ok(m) if m.is_dir() => return Err(named(path, io::Error::other("is a folder"))),
        Ok(m) => Some(m.permissions()),
        Err(_) => None,
    };
    let (mut guard, tmp) = write_temp(&target, perms, existing.as_ref(), fill)?;
    rename_replace(&tmp, &target).map_err(|e| named(path, e))?;
    guard.disarm();
    sync_dir(target.parent().unwrap_or(Path::new(".")));
    Ok(())
}

fn create_new_with(path: &Path, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(named(path, io::Error::new(io::ErrorKind::AlreadyExists, "the file exists")));
    }
    let (_guard, tmp) = write_temp(path, Perms::Normal, None, fill)?;
    // the guard removes the temporary name whether the move worked (it is gone then) or not
    rename_noreplace(&tmp, path).map_err(|e| named(path, e))?;
    sync_dir(path.parent().unwrap_or(Path::new(".")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    // spec: DATA-012
    #[test]
    fn a_lock_file_can_be_taken_once_and_again_after_it_is_dropped() {
        let dir = std::env::temp_dir().join(format!("shale-lock-test-{}", std::process::id()));
        let path = dir.join("sub").join("instance.lock");
        let first = try_lock_file(&path).unwrap();
        assert!(first.is_some());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(try_lock_file(&path).unwrap().is_none(), "a second lock must be refused");
        drop(first);
        assert!(try_lock_file(&path).unwrap().is_some(), "the lock is free after the first is dropped");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A small deterministic random generator, so the tests need no extra crate.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn bytes(&mut self) -> Vec<u8> {
            // many empty and small contents, some large; bytes are arbitrary (not UTF-8)
            let len = match self.next() % 8 {
                0 => 0,
                1 => 1,
                2 => 1 + (self.next() % 4096) as usize,
                3 => 200_000 + (self.next() % 300_000) as usize,
                _ => (self.next() % 300) as usize,
            };
            (0..len).map(|_| (self.next() >> 24) as u8).collect()
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-fsutil-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The names in a folder.
    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    // spec: DATA-004
    #[test]
    fn write_atomic_writes_and_replaces() {
        let d = tmp("basic");
        let p = d.join("a.md");
        write_atomic(&p, b"one").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        write_atomic(&p, b"two, longer").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two, longer");
        write_atomic(&p, b"x").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"x");
        assert_eq!(names(&d), vec!["a.md"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn random_contents_survive_repeated_overwrites() {
        let d = tmp("fuzz");
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let paths: Vec<PathBuf> = (0..4).map(|i| d.join(format!("note {i} ü.md"))).collect();
        let mut expect: Vec<Option<Vec<u8>>> = vec![None; paths.len()];
        for _ in 0..300 {
            let i = (rng.next() % paths.len() as u64) as usize;
            let data = rng.bytes();
            write_atomic(&paths[i], &data).unwrap();
            expect[i] = Some(data);
            // every file written so far still holds exactly its last content
            for (p, e) in paths.iter().zip(&expect) {
                match e {
                    Some(e) => assert_eq!(&fs::read(p).unwrap(), e),
                    None => assert!(!p.exists()),
                }
            }
        }
        assert_eq!(names(&d).len(), paths.len(), "no temporary files left: {:?}", names(&d));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn empty_large_and_non_utf8_contents() {
        let d = tmp("sizes");
        let p = d.join("f.bin");
        for data in [vec![], vec![0u8], vec![0xff, 0xfe, 0x00, 0xc3], vec![7u8; 5_000_000], "Müll ✓ 日本語".as_bytes().to_vec()] {
            write_atomic(&p, &data).unwrap();
            assert_eq!(read(&p).unwrap(), data);
        }
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[cfg(unix)]
    #[test]
    fn existing_permissions_are_kept_and_new_files_get_the_default() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("perms");
        let p = d.join("run.sh");
        fs::write(&p, "old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o750)).unwrap();
        write_atomic(&p, b"new").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o750);
        let q = d.join("ro.md");
        fs::write(&q, "old").unwrap();
        fs::set_permissions(&q, fs::Permissions::from_mode(0o444)).unwrap();
        write_atomic(&q, b"new").unwrap();
        assert_eq!(fs::metadata(&q).unwrap().permissions().mode() & 0o777, 0o444);
        assert_eq!(fs::read(&q).unwrap(), b"new");
        // a new file: like fs::write (0666 minus the umask), so never executable
        let n = d.join("new.md");
        write_atomic(&n, b"x").unwrap();
        let mode = fs::metadata(&n).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o111, 0);
        assert_eq!(mode & 0o600, 0o600);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("private");
        let p = d.join("sub").join("config.json");
        ensure_private_dir(p.parent().unwrap()).unwrap();
        write_atomic_private(&p, b"one").unwrap();
        write_atomic_private(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        // a file that was wider before becomes owner-only
        let w = d.join("wide");
        fs::write(&w, "x").unwrap();
        fs::set_permissions(&w, fs::Permissions::from_mode(0o644)).unwrap();
        write_atomic_private(&w, b"y").unwrap();
        assert_eq!(fs::metadata(&w).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[cfg(unix)]
    #[test]
    fn failed_writes_leave_the_original_and_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("readonly");
        let p = d.join("a.md");
        write_atomic(&p, b"precious").unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        // root ignores folder permissions, so only check when the folder really is closed
        let closed = File::create(d.join("probe")).is_err();
        let r = write_atomic(&p, b"new");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        if closed {
            assert!(r.is_err());
            assert_eq!(fs::read(&p).unwrap(), b"precious");
            assert_eq!(names(&d), vec!["a.md"]);
        }
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn errors_name_the_file_and_touch_nothing() {
        let d = tmp("errors");
        // a missing folder is an error (no folders are made), and nothing appears
        let p = d.join("no").join("such").join("a.md");
        let e = write_atomic(&p, b"x").unwrap_err();
        assert!(e.to_string().contains("a.md"), "{e}");
        assert!(!d.join("no").exists());
        // writing "over" a folder fails and leaves it alone
        let sub = d.join("folder");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inside"), "keep").unwrap();
        assert!(write_atomic(&sub, b"x").is_err());
        assert_eq!(fs::read(sub.join("inside")).unwrap(), b"keep");
        assert_eq!(names(&d), vec!["folder"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_keeps_the_link() {
        let d = tmp("symlink");
        let real = d.join("real.md");
        let link = d.join("link.md");
        fs::write(&real, "old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn create_new_never_clobbers() {
        let d = tmp("create");
        let p = d.join("a.md");
        create_new(&p, b"first").unwrap();
        let e = create_new(&p, b"second").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&p).unwrap(), b"first");
        assert_eq!(names(&d), vec!["a.md"]);
        // not even a dangling link or a folder is replaced
        #[cfg(unix)]
        {
            let l = d.join("dangling");
            std::os::unix::fs::symlink(d.join("nowhere"), &l).unwrap();
            assert!(create_new(&l, b"x").is_err());
            assert!(!d.join("nowhere").exists());
        }
        let f = d.join("folder");
        fs::create_dir(&f).unwrap();
        assert!(create_new(&f, b"x").is_err());
        assert!(f.is_dir());
        create_new(&d.join("empty"), b"").unwrap();
        assert_eq!(fs::read(d.join("empty")).unwrap(), b"");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn only_one_of_many_creators_wins() {
        let d = tmp("race-create");
        let p = Arc::new(d.join("a.md"));
        let handles: Vec<_> = (0..8u8)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || create_new(&p, &[i; 1000]).is_ok())
            })
            .collect();
        let wins = handles.into_iter().map(|h| h.join().unwrap()).filter(|w| *w).count();
        assert_eq!(wins, 1);
        let data = fs::read(&*p).unwrap();
        assert_eq!(data.len(), 1000);
        assert!(data.iter().all(|b| *b == data[0]));
        assert_eq!(names(&d), vec!["a.md"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn copy_new_copies_without_clobbering() {
        let d = tmp("copy");
        let mut rng = Rng(42);
        let data = rng.bytes();
        fs::write(d.join("src"), &data).unwrap();
        copy_new(&d.join("src"), &d.join("dest")).unwrap();
        assert_eq!(fs::read(d.join("dest")).unwrap(), data);
        assert_eq!(copy_new(&d.join("src"), &d.join("dest")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(copy_new(&d.join("missing"), &d.join("other")).is_err());
        assert!(!d.join("other").exists());
        assert_eq!(names(&d), vec!["dest", "src"]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004, DATA-013
    #[test]
    fn rename_noreplace_moves_files_and_folders_but_never_replaces() {
        let d = tmp("rename");
        fs::write(d.join("a"), "A").unwrap();
        fs::write(d.join("b"), "B").unwrap();
        assert_eq!(rename_noreplace(&d.join("a"), &d.join("b")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(d.join("a")).unwrap(), b"A");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"B");
        rename_noreplace(&d.join("a"), &d.join("c")).unwrap();
        assert_eq!(fs::read(d.join("c")).unwrap(), b"A");
        assert!(!d.join("a").exists());
        fs::create_dir(d.join("dir1")).unwrap();
        fs::create_dir(d.join("dir2")).unwrap();
        assert_eq!(rename_noreplace(&d.join("dir1"), &d.join("dir2")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        // a file onto a folder and a folder onto a file are refused too, and nothing is lost
        fs::write(d.join("f"), "F").unwrap();
        assert!(rename_noreplace(&d.join("f"), &d.join("dir2")).is_err());
        assert!(rename_noreplace(&d.join("dir2"), &d.join("f")).is_err());
        assert_eq!(fs::read(d.join("f")).unwrap(), b"F");
        assert!(d.join("dir2").is_dir());
        rename_noreplace(&d.join("dir1"), &d.join("dir3")).unwrap();
        assert!(d.join("dir3").is_dir() && !d.join("dir1").exists());
        assert!(rename_noreplace(&d.join("missing"), &d.join("x")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn a_file_another_library_wrote_can_be_synced_and_a_missing_one_is_named() {
        // Windows refuses to flush a file that was opened for reading only: a regression of the
        // mail index backup ("Access is denied")
        let d = tmp("sync");
        fs::write(d.join("backup.sqlite"), "data").unwrap();
        sync_file(&d.join("backup.sqlite")).unwrap();
        assert!(sync_file(&d.join("missing.sqlite")).unwrap_err().to_string().contains("missing.sqlite"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-013
    #[test]
    fn the_atomic_save_replaces_but_the_no_replace_rename_does_not() {
        // std::fs::rename replaces the target on Windows and Unix alike: that is what a save
        // needs, and what rename_noreplace must never do
        let d = tmp("replace-vs-noreplace");
        fs::write(d.join("old"), "old").unwrap();
        fs::write(d.join("new"), "new").unwrap();
        fs::rename(d.join("new"), d.join("old")).unwrap();
        assert_eq!(fs::read(d.join("old")).unwrap(), b"new");
        fs::write(d.join("again"), "again").unwrap();
        assert_eq!(rename_noreplace(&d.join("again"), &d.join("old")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(d.join("old")).unwrap(), b"new");
        assert_eq!(fs::read(d.join("again")).unwrap(), b"again");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-014
    #[cfg(windows)]
    #[test]
    fn private_files_on_windows_carry_no_access_entry_for_other_users() {
        let d = tmp("acl");
        let p = d.join("sub").join("config.json");
        ensure_private_dir(p.parent().unwrap()).unwrap();
        write_atomic_private(&p, b"secret").unwrap();
        let root = private_temp_root("shale-acl-test").unwrap();
        // Shale's own folders are in the user's profile, where only the user, SYSTEM and the
        // administrators have access by default
        // (compared after canonicalize: the temp folder may be spelled with 8.3 short names)
        let real = |p: PathBuf| fs::canonicalize(&p).unwrap_or(p);
        let home = real(dirs::home_dir().unwrap());
        assert!(real(dirs::config_dir().unwrap()).starts_with(&home), "the config folder is in the profile");
        assert!(real(dirs::data_dir().unwrap()).starts_with(&home), "the data folder is in the profile");
        assert!(real(std::env::temp_dir()).starts_with(&home), "the temp folder is in the profile");
        for path in [&p, p.parent().unwrap(), &root] {
            let out = std::process::Command::new("icacls").arg(path).output().expect("icacls");
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            assert!(out.status.success(), "{text}");
            for other in ["Everyone", "BUILTIN\\Users", "Authenticated Users"] {
                assert!(!text.contains(other), "{} gives access to {other}:\n{text}", path.display());
            }
        }
        let _ = fs::remove_dir(&root);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn reads_report_errors_with_the_name() {
        let d = tmp("read");
        let e = read(d.join("missing.md")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(e.to_string().contains("missing.md"));
        fs::write(d.join("bad.md"), [0x66, 0xff, 0xfe]).unwrap();
        let e = read_to_string(d.join("bad.md")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("bad.md"));
        assert_eq!(read(d.join("bad.md")).unwrap(), vec![0x66, 0xff, 0xfe]);
        assert!(read(&d).is_err(), "a folder is not readable as a file");
        fs::write(d.join("ok.md"), "Müll").unwrap();
        assert_eq!(read_to_string(d.join("ok.md")).unwrap(), "Müll");
        fs::write(d.join("empty.md"), "").unwrap();
        assert_eq!(read_to_string(d.join("empty.md")).unwrap(), "");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-004
    #[test]
    fn concurrent_writers_never_produce_a_torn_file() {
        let d = tmp("race");
        let p = Arc::new(d.join("shared.md"));
        let mut rng = Rng(7);
        // each writer has its own content (different sizes), so a mix would be visible
        let contents: Arc<Vec<Vec<u8>>> = Arc::new((0..6u8).map(|i| vec![i + 1; 1 + (rng.next() % 400_000) as usize]).collect());
        write_atomic(&p, &contents[0]).unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let (p, c, stop) = (p.clone(), contents.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut seen = 0;
                while !stop.load(Ordering::Relaxed) {
                    let data = read(&*p).unwrap();
                    assert!(c.contains(&data), "torn read of {} bytes", data.len());
                    seen += 1;
                }
                seen
            })
        };
        let writers: Vec<_> = (0..6)
            .map(|i| {
                let (p, c) = (p.clone(), contents.clone());
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        write_atomic(&p, &c[i]).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0);
        let last = fs::read(&*p).unwrap();
        assert!(contents.contains(&last));
        assert_eq!(names(&d), vec!["shared.md"], "no temporary files left");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn temporary_names_are_unique() {
        let d = tmp("names");
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for _ in 0..50 {
            let (f, p) = make_temp(&d.join("x"), Perms::Normal).unwrap();
            assert!(seen.insert(p.clone()));
            files.push((f, p));
        }
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-010
    #[test]
    fn no_unsafe_file_calls_outside_fsutil() {
        let banned = ["fs::write(", "File::create(", "File::open(", "OpenOptions", "fs::copy(", "fs::rename(", "fs::read(", "fs::read_to_string("];
        let mut bad = Vec::new();
        let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for e in fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with(".rs") || name == "fsutil.rs" || name.ends_with("_tests.rs") {
                    continue;
                }
                let text = fs::read_to_string(&p).unwrap();
                // everything from the test module on is test code
                let code = text.split("#[cfg(test)]").next().unwrap_or("");
                for (i, line) in code.lines().enumerate() {
                    let l = line.trim_start();
                    if l.starts_with("//") {
                        continue;
                    }
                    if banned.iter().any(|b| l.contains(b)) {
                        bad.push(format!("{}:{}: {}", p.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display(), i + 1, l));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "write user data through fsutil instead:\n{}", bad.join("\n"));
    }
}
