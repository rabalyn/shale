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

#[cfg(windows)]
mod winacl;

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
/// The error says which file could not be opened. (The Thunderbird lookup of dropped messages
/// and the sound decoder use it.)
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

/// Make a folder (and its parents) that only the owner can enter (mode 0700; on Windows a
/// protected access list for the user and `SYSTEM`, DATA-014). A folder that exists gets its
/// permissions tightened.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    winacl::restrict_to_owner(path, true).map_err(|e| named(path, e))?;
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
    #[cfg(not(unix))]
    let b = fs::DirBuilder::new();
    b.create(path).map_err(|e| named(path, e))?;
    // Windows: the folder is empty while it still has the access list of its place (DATA-014)
    #[cfg(windows)]
    if let Err(e) = winacl::restrict_to_owner(path, true) {
        let _ = fs::remove_dir(path);
        return Err(named(path, e));
    }
    Ok(())
}

/// May `folder` be removed as the managed folder of a tool Shale installs below `<data>/shale`
/// (`what`: "the managed TeX Live")? Only one of the `allowed` folders, which is a real folder: a
/// link is never followed, and neither is a link in the place of `<data>/shale` (TEX-003, SYNC-028).
pub fn removable_folder(data: &Path, folder: &Path, allowed: &[PathBuf], what: &str) -> Result<(), String> {
    if !allowed.iter().any(|a| a == folder) {
        return Err(format!("{} is not the folder of {what}", folder.display()));
    }
    let real_dir = |p: &Path| fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
    if !real_dir(&data.join("shale")) {
        return Err(format!("{} is not a plain folder", data.join("shale").display()));
    }
    if !real_dir(folder) {
        return Err(format!("{} is not a plain folder (a link, or a file): nothing was removed", folder.display()));
    }
    Ok(())
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
    #[cfg(windows)]
    winacl::restrict_to_owner(path, false).map_err(|e| named(path, e))?;
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
    {
        let f = o.open(path)?;
        #[cfg(windows)]
        winacl::restrict_to_owner(path, false)?;
        Ok(f)
    }
}

/// Move a file or folder to `to`, which must not exist (`AlreadyExists` otherwise). Unlike a
/// plain rename this can never replace something at the target, even if it appeared a moment
/// ago. Where the file system can't promise that, it falls back to checking first. A rename that
/// only changes the letter case of the name on a file system that ignores it (macOS, Windows:
/// `to` "exists" because it is `from` itself) goes through a temporary name (DATA-013).
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    if is_case_only_rename(from, to) {
        return rename_through_temp_name(from, to);
    }
    rename_noreplace_once(from, to)
}

fn rename_noreplace_once(from: &Path, to: &Path) -> io::Result<()> {
    match renameat2_noreplace(from, to) {
        Some(r) => r,
        None => rename_noreplace_fallback(from, to),
    }
}

/// Whether `to` is `from` itself in another letter case on a file system that ignores letter case
/// (macOS, Windows): the same folder, names that differ only in letter case, `to` is found, and
/// the folder lists `from` by its own spelling but nothing spelled like `to`. On a file system
/// that tells `plan.md` from `Plan.md` this is never true: a `to` that is found is listed.
pub fn is_case_only_rename(from: &Path, to: &Path) -> bool {
    let (Some(dir), Some(a), Some(b)) = (from.parent(), from.file_name(), to.file_name()) else { return false };
    if to.parent() != Some(dir) || !differs_only_in_case(&a.to_string_lossy(), &b.to_string_lossy()) || fs::symlink_metadata(to).is_err() {
        return false;
    }
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(entries) = fs::read_dir(dir) else { return false };
    let listed: Vec<std::ffi::OsString> = entries.flatten().map(|e| e.file_name()).collect();
    listed.iter().any(|n| n == a) && !listed.iter().any(|n| n == b)
}

/// Two names that are not the same but equal when letter case is ignored.
fn differs_only_in_case(a: &str, b: &str) -> bool {
    a != b && a.to_lowercase() == b.to_lowercase()
}

/// `from` → a free temporary name in its folder → `to`. If the second step fails the first is
/// undone.
fn rename_through_temp_name(from: &Path, to: &Path) -> io::Result<()> {
    let dir = from.parent().unwrap_or(Path::new(""));
    let tmp = (0..100)
        .map(|_| dir.join(format!(".shale-tmp-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))))
        .find(|t| fs::symlink_metadata(t).is_err())
        .ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, "no free temporary name"))?;
    rename_noreplace_once(from, &tmp)?;
    rename_noreplace_once(&tmp, to).inspect_err(|_| {
        let _ = rename_noreplace_once(&tmp, from);
    })
}

/// The way without `renameat2`: a hard link fails if the target exists (files only), else check
/// first and rename.
fn rename_noreplace_fallback(from: &Path, to: &Path) -> io::Result<()> {
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
    let (a, b) = (win32_path(from), win32_path(to));
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

/// A path for a Win32 call, NUL-terminated. A long one (from 248 UTF-16 units, where folder calls
/// already fail; `std::fs` draws the line there as well) is made absolute and gets the `\\?\`
/// prefix, without which Windows refuses paths over 260 characters (`MAX_PATH`, DATA-013): a
/// vault deep in a profile folder with a 200-byte note name is over it.
#[cfg(windows)]
fn win32_path(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = p.as_os_str().encode_wide().collect();
    let mut out = if wide.len() < 248 {
        wide
    } else {
        match std::path::absolute(p) {
            Ok(abs) => verbatim(&abs.as_os_str().encode_wide().collect::<Vec<u16>>()),
            Err(_) => wide,
        }
    };
    out.push(0);
    out
}

/// An absolute Windows path (UTF-16) with the `\\?\` prefix: `C:\x` → `\\?\C:\x`, `\\server\share`
/// → `\\?\UNC\server\share`; a path that has a `\\?\` or `\\.\` prefix already stays as it is.
/// Expects what `std::path::absolute` gives on Windows (backslashes, no `.` or `..`).
#[cfg(any(windows, test))]
fn verbatim(abs: &[u16]) -> Vec<u16> {
    let starts = |pre: &str| abs.starts_with(&pre.encode_utf16().collect::<Vec<u16>>());
    if starts(r"\\?\") || starts(r"\\.\") {
        abs.to_vec()
    } else if starts(r"\\") {
        r"\\?\UNC\".encode_utf16().chain(abs[2..].iter().copied()).collect()
    } else {
        r"\\?\".encode_utf16().chain(abs.iter().copied()).collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn renameat2_noreplace(_: &Path, _: &Path) -> Option<io::Result<()>> {
    None
}

/// A path as people and Windows programs write it: `canonicalize` makes `\\?\C:\x` of `C:\x`, and
/// Shale never stores or shows that prefix. On other systems the path is returned as it is.
pub fn plain_path(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        if let Some(p) = without_verbatim(&path.to_string_lossy()) {
            return PathBuf::from(p);
        }
    }
    path
}

/// `C:\x` for `\\?\C:\x` and `\\server\share\x` for `\\?\UNC\server\share\x`; None for any other
/// path (also for the verbatim names of devices and volume GUIDs, which have no ordinary form).
/// Pure text, so it can be tested on every system.
pub fn without_verbatim(p: &str) -> Option<String> {
    if let Some(unc) = p.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{unc}"));
    }
    let drive = p.strip_prefix(r"\\?\")?;
    let b = drive.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':').then(|| drive.to_string())
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
        match o.open(&tmp) {
            // Windows: still empty, so nothing is readable before the access list is owner-only (DATA-014)
            #[cfg(windows)]
            Ok(f) if perms == Perms::Private => match winacl::restrict_to_owner(&tmp, false) {
                Ok(()) => return Ok((f, tmp)),
                Err(e) => {
                    drop(f);
                    let _ = fs::remove_file(&tmp);
                    return Err(named(target, e));
                }
            },
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
/// violation" and works a moment later, so it is tried again for about a second. Also the way an
/// update puts the program files in place (UPD-014 to UPD-017; a running program can be replaced
/// like this on Linux and macOS).
pub fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
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

/// A file or folder name that works on Linux, macOS and Windows (VAULT-026): the characters that
/// Windows forbids (`< > : " / \ | ? *`) and control characters become `-`, trailing dots and
/// spaces are trimmed, a Windows device name (`CON`, `NUL`, `COM1`, …, any case, with or without
/// an extension) gets `_` added after its stem, and the name is cut at a character boundary to
/// `max_bytes` bytes (UTF-8) with its extension kept. A name with nothing left stays empty: the
/// caller chooses what to call it.
pub fn portable_name(name: &str, max_bytes: usize) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '-' } else { c }).collect();
    let clean = clean.trim_end_matches(['.', ' ']);
    let mut budget = max_bytes;
    loop {
        let cut = cut_keeping_extension(clean, budget);
        let fixed = avoid_device_name(&cut);
        if fixed.len() <= max_bytes || budget == 0 {
            return fixed;
        }
        budget -= 1;
    }
}

/// The longest name the user types for a note, folder or file (VAULT-027).
pub const TYPED_NAME_MAX: usize = 200;
/// The longest name Shale makes itself (imports, the media folder; VAULT-026).
pub const MADE_NAME_MAX: usize = 127;

/// What about `name` would not work on Windows or on a file system with a shorter limit, one
/// sentence each (VAULT-027, VAULT-028): `` `:` is not allowed on Windows``. Empty when
/// [`portable_name`] would leave the name as it is (the outer spaces do not count: they are always
/// trimmed).
pub fn portable_problems(name: &str, max_bytes: usize) -> Vec<String> {
    let name = name.trim();
    let mut out = Vec::new();
    let mut bad: Vec<char> = Vec::new();
    for c in name.chars().filter(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')) {
        if !bad.contains(&c) {
            bad.push(c);
        }
    }
    if !bad.is_empty() {
        let list = bad.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(" ");
        out.push(format!("{list} {} not allowed on Windows", if bad.len() == 1 { "is" } else { "are" }));
    }
    if name.chars().any(char::is_control) {
        out.push("control characters are not allowed on Windows".into());
    }
    if name.ends_with('.') {
        out.push("a name can't end in a dot on Windows".into());
    }
    let device = avoid_device_name(name);
    if device != name {
        let base = name.split('.').next().unwrap_or("").trim_end();
        out.push(format!("`{base}` is a reserved name on Windows"));
    }
    if name.len() > max_bytes {
        out.push(format!("the name is longer than {max_bytes} bytes"));
    }
    out
}

/// The toast for a name the user typed that [`portable_name`] had to change, `None` when nothing
/// had to change (VAULT-027): "Saved as `a-b.md`: `:` is not allowed on Windows". `typed` may be a
/// path (each part is looked at); `got` is the name the file really got; `verb` starts the text
/// (`Saved as`, `Renamed to`).
pub fn portable_notice(verb: &str, typed: &str, got: &str, max_bytes: usize) -> Option<String> {
    let mut problems: Vec<String> = Vec::new();
    for part in typed.split('/') {
        for p in portable_problems(part, max_bytes) {
            if !problems.contains(&p) {
                problems.push(p);
            }
        }
    }
    (!problems.is_empty()).then(|| format!("{verb} `{got}`: {}", problems.join("; ")))
}

/// `name` cut to `max` bytes at a character boundary, the extension (from the last dot) kept when
/// it is shorter than `max`.
fn cut_keeping_extension(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i < max => name.split_at(i),
        _ => (name, ""),
    };
    let mut end = max - ext.len();
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ext}", stem[..end].trim_end_matches(['.', ' ']))
}

/// `NUL` → `NUL_`, `con.txt` → `con_.txt`: Windows treats the part before the first dot as a device.
fn avoid_device_name(name: &str) -> String {
    let base = name.split('.').next().unwrap_or("");
    let upper = base.trim_end().to_ascii_uppercase();
    let device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (upper.len() == 4 && (upper.starts_with("COM") || upper.starts_with("LPT")) && matches!(upper.as_bytes()[3], b'1'..=b'9'));
    if device {
        format!("{base}_{}", &name[base.len()..])
    } else {
        name.to_string()
    }
}

/// A Unix socket listening at `path`, however long the path is (a test helper).
///
/// A socket path has a small limit (`sun_path`: 104 bytes on macOS, 108 on Linux), and macOS's
/// temp folder (`/var/folders/xx/…/T/`) alone takes about 49 of them, so a test that binds a socket
/// deep in a temp tree panics with "path must be shorter than SUN_LEN" there (#551). The limit is on
/// the string given to `bind`, not on where the socket ends up, and the kernel resolves symbolic
/// links in the folders of that string. So the socket is bound through a short link,
/// `/tmp/shale-s<pid>-<n>`, to the folder of `path`: it is created in the real folder, under its
/// name, and the link is removed straight away (the listener does not need the name it was bound
/// by). `path`'s folder must exist; `/tmp` is tried first, then the system temp folder, when that
/// is short enough.
///
/// # Panics
/// When the folder cannot be resolved, or no short enough link can be made, or `bind` fails.
#[cfg(test)]
#[cfg(unix)]
pub(crate) fn bind_unix_socket(path: &Path) -> std::os::unix::net::UnixListener {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    const SHORT: usize = 90; // under SUN_LEN of every Unix, with room for the NUL
    let name = path.file_name().expect("a socket path has a file name");
    let folder = path.parent().expect("a socket path has a folder").canonicalize().expect("the folder of the socket exists");
    let link_name = format!("shale-s{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    for base in [PathBuf::from("/tmp"), std::env::temp_dir()] {
        let link = base.join(&link_name);
        if link.join(name).as_os_str().len() >= SHORT || std::os::unix::fs::symlink(&folder, &link).is_err() {
            continue;
        }
        let bound = std::os::unix::net::UnixListener::bind(link.join(name));
        let _ = fs::remove_file(&link);
        return bound.unwrap_or_else(|e| panic!("cannot bind the socket {} through {}: {e}", path.display(), link.display()));
    }
    panic!("no short folder to make a link in (/tmp and {} do not work), for the socket {}", std::env::temp_dir().display(), path.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[cfg(unix)]
    #[test]
    fn a_socket_is_bound_at_a_long_path_through_a_short_link() {
        use std::os::unix::fs::FileTypeExt;
        let long = std::env::temp_dir().join(format!("shale-sock-test-{}-{}", std::process::id(), "x".repeat(120))).join("inner");
        fs::create_dir_all(&long).unwrap();
        let path = long.join("s");
        assert!(path.as_os_str().len() > 120, "longer than any socket path may be");
        assert!(std::os::unix::net::UnixListener::bind(&path).is_err(), "the plain bind refuses it");
        let listener = bind_unix_socket(&path);
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_socket(), "the socket is at the long path");
        drop(listener);
        let leftovers = fs::read_dir("/tmp").unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&format!("shale-s{}-", std::process::id()))).count();
        assert_eq!(leftovers, 0, "the link is gone");
        fs::remove_dir_all(long.parent().unwrap()).unwrap();
    }

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
        // Another test thread that starts a program (gpg, a helper process) forks: until the child's
        // exec closes it, the child shares our descriptor and so still holds the lock for a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let again = loop {
            match try_lock_file(&path).unwrap() {
                Some(lock) => break Some(lock),
                None if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
                None => break None,
            }
        };
        assert!(again.is_some(), "the lock is free after the first is dropped");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_private_log_is_started_empty_or_appended_and_owner_only() {
        let d = tmp("log");
        let p = d.join("sub").join("x.log");
        use std::io::Write;
        open_private_log(&p, false).unwrap().write_all(b"one").unwrap();
        open_private_log(&p, true).unwrap().write_all(b"two").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "onetwo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // an older, wider file is narrowed again
            fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
            drop(open_private_log(&p, true).unwrap());
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        open_private_log(&p, false).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "", "started empty");
        let _ = fs::remove_dir_all(&d);
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

    // spec: DATA-004, DATA-014
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

    // spec: DATA-013
    #[test]
    fn a_rename_that_only_changes_letter_case_works_also_where_letter_case_is_ignored() {
        let d = tmp("case-rename");
        fs::write(d.join("plan.md"), "P").unwrap();
        fs::create_dir(d.join("notes")).unwrap();
        fs::write(d.join("notes").join("x"), "X").unwrap();
        // on every system: afterwards the folder lists the new spelling, and the content is kept
        rename_noreplace(&d.join("plan.md"), &d.join("Plan.md")).unwrap();
        rename_noreplace(&d.join("notes"), &d.join("Notes")).unwrap();
        assert_eq!(names(&d), vec!["Notes", "Plan.md"]);
        assert_eq!(fs::read(d.join("Plan.md")).unwrap(), b"P");
        assert_eq!(fs::read(d.join("Notes").join("x")).unwrap(), b"X");
        // the two steps on their own (macOS and Windows take them): the temporary name is gone
        rename_through_temp_name(&d.join("Plan.md"), &d.join("PLAN.md")).unwrap();
        assert_eq!(names(&d), vec!["Notes", "PLAN.md"]);
        // a second step that fails is undone: nothing is replaced and nothing is lost
        fs::write(d.join("other"), "O").unwrap();
        assert_eq!(rename_through_temp_name(&d.join("PLAN.md"), &d.join("other")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(names(&d), vec!["Notes", "PLAN.md", "other"]);
        assert_eq!((fs::read(d.join("PLAN.md")).unwrap(), fs::read(d.join("other")).unwrap()), (b"P".to_vec(), b"O".to_vec()));
        // another name, another folder, or a target that is not there is no case-only rename
        assert!(!is_case_only_rename(&d.join("PLAN.md"), &d.join("other")));
        assert!(!is_case_only_rename(&d.join("PLAN.md"), &d.join("Notes").join("plan.md")));
        assert!(!is_case_only_rename(&d.join("PLAN.md"), &d.join("plan 2.md")));
        // where letter case counts, two real files are two names, and one never replaces the other
        if fs::write(d.join("plan.md"), "Q").is_ok() && names(&d).len() == 4 {
            assert!(!is_case_only_rename(&d.join("PLAN.md"), &d.join("plan.md")));
            assert_eq!(rename_noreplace(&d.join("PLAN.md"), &d.join("plan.md")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(d.join("plan.md")).unwrap(), b"Q");
        } else {
            // where it does not, the other spelling is the same file
            assert!(is_case_only_rename(&d.join("PLAN.md"), &d.join("plan.md")));
            println!("this file system ignores letter case: the part with two files that differ in it is skipped");
        }
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-013
    #[test]
    fn rename_noreplace_works_on_paths_longer_than_260_characters() {
        // Windows refuses a path over 260 characters without the \\?\ prefix
        let d = tmp("rename-long");
        let deep = d.join("f".repeat(120)).join("g".repeat(120));
        fs::create_dir_all(&deep).unwrap();
        let (a, b) = (deep.join(format!("{}.md", "a".repeat(150))), deep.join(format!("{}.md", "b".repeat(150))));
        assert!(b.as_os_str().len() > 300);
        create_new(&a, b"A").unwrap();
        rename_noreplace(&a, &b).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"A");
        create_new(&a, b"A2").unwrap();
        assert_eq!(rename_noreplace(&a, &b).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&b).unwrap(), b"A");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-013
    #[test]
    fn a_long_windows_path_gets_the_verbatim_prefix_once() {
        let v = |p: &str| String::from_utf16(&verbatim(&p.encode_utf16().collect::<Vec<u16>>())).unwrap();
        assert_eq!(v(r"C:\Users\me\vault\note.md"), r"\\?\C:\Users\me\vault\note.md");
        assert_eq!(v(r"\\server\share\vault\note.md"), r"\\?\UNC\server\share\vault\note.md");
        assert_eq!(v(r"\\?\C:\already"), r"\\?\C:\already");
        assert_eq!(v(r"\\.\pipe\x"), r"\\.\pipe\x");
    }

    // spec: MEDIA-020
    #[test]
    fn the_verbatim_prefix_of_a_canonical_windows_path_is_removed_where_the_rest_is_an_ordinary_path() {
        assert_eq!(without_verbatim(r"\\?\C:\Users\a.mp4").as_deref(), Some(r"C:\Users\a.mp4"));
        assert_eq!(without_verbatim(r"\\?\d:\").as_deref(), Some(r"d:\"));
        assert_eq!(without_verbatim(r"\\?\UNC\nas\share\a.mp4").as_deref(), Some(r"\\nas\share\a.mp4"));
        for same in [r"C:\Users\a.mp4", r"\\nas\share\a.mp4", r"\\?\Volume{0f1e}\a", r"\\?\pipe\x", r"\\.\C:\x", r"\\?\C", "/home/ada/a.mp4", ""] {
            assert_eq!(without_verbatim(same), None, "{same}");
        }
        // plain_path acts on Windows only: a Unix name may well start with a backslash
        let odd = PathBuf::from(r"\\?\C:\x");
        assert_eq!(plain_path(odd.clone()), if cfg!(windows) { PathBuf::from(r"C:\x") } else { odd });
    }

    // spec: DATA-004
    #[test]
    fn the_fallback_without_renameat2_has_the_same_no_replace_guarantee() {
        let d = tmp("rename-fallback");
        fs::write(d.join("a"), "A").unwrap();
        fs::write(d.join("b"), "B").unwrap();
        // name taken: nothing moves, nothing is replaced
        assert_eq!(rename_noreplace_fallback(&d.join("a"), &d.join("b")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(d.join("a")).unwrap(), b"A");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"B");
        // free name: the file is moved (no second name is left behind)
        rename_noreplace_fallback(&d.join("a"), &d.join("c")).unwrap();
        assert_eq!(fs::read(d.join("c")).unwrap(), b"A");
        assert_eq!(names(&d), vec!["b", "c"]);
        // folders: no hard link, so the exists check decides (also for an empty target folder)
        fs::create_dir(d.join("dir1")).unwrap();
        fs::write(d.join("dir1/x"), "X").unwrap();
        fs::create_dir(d.join("dir2")).unwrap();
        assert_eq!(rename_noreplace_fallback(&d.join("dir1"), &d.join("dir2")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        rename_noreplace_fallback(&d.join("dir1"), &d.join("dir3")).unwrap();
        assert_eq!(fs::read(d.join("dir3/x")).unwrap(), b"X");
        assert!(!d.join("dir1").exists());
        // a file onto a folder, a missing source, a missing target folder
        assert_eq!(rename_noreplace_fallback(&d.join("b"), &d.join("dir2")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(rename_noreplace_fallback(&d.join("missing"), &d.join("y")).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(rename_noreplace_fallback(&d.join("b"), &d.join("nodir/z")).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(d.join("b")).unwrap(), b"B");
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
        // and since DATA-014 the list is Shale's own: protected, the user and SYSTEM only
        for path in [&p, p.parent().unwrap(), &root] {
            winacl::assert_owner_only(path);
        }
        let _ = fs::remove_dir(&root);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-014
    #[cfg(windows)]
    #[test]
    fn private_files_on_windows_have_a_protected_access_list_for_the_user_and_system_only() {
        use winacl::{assert_only_user_and_system, assert_owner_only};
        let d = tmp("acl-own");
        // an empty folder that exists has the list of its place (open to others as far as that goes): it is closed
        let old = d.join("old");
        fs::create_dir(&old).unwrap();
        ensure_private_dir(&old).unwrap();
        assert_owner_only(&old);
        // a file made by an atomic save is protected itself, and keeps the list over a second save
        let settings = old.join("settings.json");
        write_atomic_private(&settings, b"one").unwrap();
        write_atomic_private(&settings, b"two").unwrap();
        assert_owner_only(&settings);
        assert_eq!(fs::read(&settings).unwrap(), b"two");
        // a folder made in one go; what is made in it has the same two entries, inherited
        let made = d.join("made");
        create_private_dir_new(&made).unwrap();
        assert_owner_only(&made);
        fs::write(made.join("in.txt"), b"x").unwrap();
        assert_only_user_and_system(&made.join("in.txt"), false);
        assert!(create_private_dir_new(&made).is_err(), "one that is there is never adopted");
        // the lock file with its folder, and a log, also one that existed with the list of its place
        let lock = d.join("data").join("instance.lock");
        let held = try_lock_file(&lock).unwrap().expect("the lock is free");
        assert_owner_only(&lock);
        assert_owner_only(lock.parent().unwrap());
        drop(held);
        let log = d.join("logs").join("a.log");
        drop(open_private_log(&log, false).unwrap());
        assert_owner_only(&log);
        let old_log = d.join("old.log");
        fs::write(&old_log, "old").unwrap();
        drop(open_private_log(&old_log, true).unwrap());
        assert_owner_only(&old_log);
        // the private temp root
        let root = private_temp_root("shale-acl-own-test").unwrap();
        assert_owner_only(&root);
        let _ = fs::remove_dir(&root);
        // a folder that has files keeps its list (setting one would walk over everything in it)
        let full = d.join("full");
        fs::create_dir(&full).unwrap();
        fs::write(full.join("a.md"), b"x").unwrap();
        ensure_private_dir(&full).unwrap();
        assert!(!winacl::entries(&full).0, "a folder with files keeps the list of its place");
        // an ordinary save does not change the list of its place
        let plain = d.join("plain.md");
        write_atomic(&plain, b"note").unwrap();
        assert!(!winacl::entries(&plain).0, "an ordinary file keeps the list of its place");
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
        // all seven threads start together, so the reader runs while the writers write
        let start = Arc::new(std::sync::Barrier::new(7));
        let reader = {
            let (p, c, stop, start) = (p.clone(), contents.clone(), stop.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                let mut seen = 0;
                // at least one read even if the writers were done first on a busy machine
                while seen == 0 || !stop.load(Ordering::Relaxed) {
                    let data = read(&*p).unwrap();
                    assert!(c.contains(&data), "torn read of {} bytes", data.len());
                    seen += 1;
                }
                seen
            })
        };
        let writers: Vec<_> = (0..6)
            .map(|i| {
                let (p, c, start) = (p.clone(), contents.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
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

    /// The lines (1-based, trimmed) of one source file that call a file function DATA-010 bans. Code compiled
    /// only for tests is exempt: `*_tests.rs` files, everything after the first `#[cfg(test)]`, and a file
    /// whose inner attributes (before any code) include `#![cfg(test)]`.
    fn unsafe_file_calls(name: &str, text: &str) -> Vec<(usize, String)> {
        let banned = ["fs::write(", "File::create(", "File::open(", "OpenOptions", "fs::copy(", "fs::rename(", "fs::read(", "fs::read_to_string("];
        if name.ends_with("_tests.rs") {
            return Vec::new();
        }
        for line in text.lines() {
            let l = line.trim();
            if l == "#![cfg(test)]" {
                return Vec::new();
            }
            if !(l.is_empty() || l.starts_with("//") || l.starts_with("#![")) {
                break; // the first code line ends the inner attributes
            }
        }
        // everything from the test module on is test code
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        let mut bad = Vec::new();
        for (i, line) in code.lines().enumerate() {
            let l = line.trim_start();
            if !l.starts_with("//") && banned.iter().any(|b| l.contains(b)) {
                bad.push((i + 1, l.to_string()));
            }
        }
        bad
    }

    // spec: DATA-010
    #[test]
    fn no_unsafe_file_calls_outside_fsutil() {
        let mut bad = Vec::new();
        let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for e in fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    // the benchmark harness (BUILD-038) writes generated data into temporary folders and its own result files: no user data
                    if p.file_name().is_some_and(|n| n != "bench") {
                        stack.push(p);
                    }
                    continue;
                }
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                // console.rs opens the devices of the terminal (`CONOUT$`), not user data (BUILD-042)
                if !name.ends_with(".rs") || name == "fsutil.rs" || name == "console.rs" {
                    continue;
                }
                let text = fs::read_to_string(&p).unwrap();
                for (line, l) in unsafe_file_calls(&name, &text) {
                    bad.push(format!("{}:{}: {}", p.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().display(), line, l));
                }
            }
        }
        assert!(bad.is_empty(), "write user data through fsutil instead:\n{}", bad.join("\n"));
    }

    // spec: DATA-010
    #[test]
    fn data_010_test_only_files_are_exempt_and_others_are_not() {
        let call = "    let _ = std::fs::read_to_string(p);\n";
        let plain = format!("//! docs\n\nfn f() {{\n{call}}}\n");
        assert_eq!(unsafe_file_calls("a.rs", &plain).len(), 1, "a normal file is flagged");
        let inner = format!("//! docs\n#![cfg(test)]\n\nfn f() {{\n{call}}}\n");
        assert!(unsafe_file_calls("a.rs", &inner).is_empty(), "#![cfg(test)] exempts the file");
        assert!(unsafe_file_calls("a_tests.rs", &plain).is_empty(), "*_tests.rs stays exempt");
        let after = format!("fn f() {{}}\n#[cfg(test)]\nmod t {{\n{call}}}\n");
        assert!(unsafe_file_calls("a.rs", &after).is_empty(), "code after #[cfg(test)] stays exempt");
        let in_comment = format!("//! docs\n// #![cfg(test)]\nfn f() {{\n{call}}}\n");
        assert_eq!(unsafe_file_calls("a.rs", &in_comment).len(), 1, "in a comment it does not count");
        let in_string = format!("fn f() {{\n    let s = \"#![cfg(test)]\";\n{call}}}\n");
        assert_eq!(unsafe_file_calls("a.rs", &in_string).len(), 1, "in a string it does not count");
        let later = format!("fn f() {{}}\n#![cfg(test)]\nfn g() {{\n{call}}}\n");
        assert_eq!(unsafe_file_calls("a.rs", &later).len(), 1, "after code it does not count");
    }

    // spec: VAULT-026
    #[test]
    fn portable_names_have_no_forbidden_or_control_characters() {
        assert_eq!(portable_name("a<b>c:d\"e/f\\g|h?i*j", 127), "a-b-c-d-e-f-g-h-i-j");
        assert_eq!(portable_name("tab\there\nnew\u{7f}del\u{85}x", 127), "tab-here-new-del-x");
        assert_eq!(portable_name("Bericht 2026 – Übersicht.docx", 127), "Bericht 2026 – Übersicht.docx", "everything else stays");
    }

    // spec: VAULT-026
    #[test]
    fn portable_names_lose_trailing_dots_and_spaces() {
        assert_eq!(portable_name("name. . ", 127), "name");
        assert_eq!(portable_name("a.txt.", 127), "a.txt");
        assert_eq!(portable_name(".hidden", 127), ".hidden", "a leading dot stays");
        assert_eq!(portable_name("...", 127), "");
        assert_eq!(portable_name("  ", 127), "");
    }

    // spec: VAULT-026
    #[test]
    fn portable_names_avoid_windows_device_names_in_any_case_with_or_without_an_extension() {
        for d in ["CON", "PRN", "AUX", "NUL", "COM1", "COM9", "LPT1", "LPT9", "con", "Nul", "cOm5", "lpt3"] {
            assert_eq!(portable_name(d, 127), format!("{d}_"), "{d}");
            assert_eq!(portable_name(&format!("{d}.txt"), 127), format!("{d}_.txt"), "{d}.txt");
        }
        assert_eq!(portable_name("aux.tar.gz", 127), "aux_.tar.gz", "the part before the first dot decides");
        for ok in ["COM0", "COM10", "COMX", "CONSOLE", "LPT", "console.txt", "xcon", "a.con"] {
            assert_eq!(portable_name(ok, 127), ok, "{ok} is no device name");
        }
    }

    // spec: VAULT-026
    #[test]
    fn portable_names_are_cut_at_a_character_boundary_and_keep_their_extension() {
        let long = "a".repeat(300);
        assert_eq!(portable_name(&long, 127).len(), 127);
        let with_ext = format!("{long}.docx");
        let cut = portable_name(&with_ext, 127);
        assert_eq!(cut.len(), 127);
        assert!(cut.ends_with(".docx") && cut.starts_with("aaa"), "{cut}");
        // two-byte letters: never cut in the middle of one
        let umlauts = "ä".repeat(100);
        let cut = portable_name(&format!("{umlauts}.md"), 127);
        assert_eq!(cut, format!("{}.md", "ä".repeat(62)), "62 letters = 124 bytes + 3");
        assert!(cut.len() <= 127);
        let cut = portable_name(&"€".repeat(100), 10);
        assert_eq!(cut, "€€€", "three bytes each");
        // the stem does not end in a dot or space after the cut
        assert_eq!(portable_name("ab c.txt", 6), "ab.txt", "'ab ' loses its space before the extension");
        // an extension as long as the limit is not kept
        assert_eq!(portable_name("a.0123456789", 5), "a.012");
        // a name that is short enough is not touched
        assert_eq!(portable_name("short.md", 127), "short.md");
    }

    // spec: VAULT-026
    #[test]
    fn a_cut_that_makes_a_device_name_or_the_added_underscore_still_fits() {
        assert_eq!(portable_name("CONSOLE.txt", 7), "CO.txt", "cut to CON.txt, then CON_.txt is one byte over: cut again");
        assert_eq!(portable_name("CONSOLE", 3), "CO");
        assert!(portable_name("NULL.txt", 7).len() <= 7);
        assert_eq!(portable_name("x", 0), "");
        assert_eq!(portable_name("", 127), "");
    }

    // spec: VAULT-027
    #[test]
    fn portable_problems_name_each_reason_and_nothing_for_a_good_name() {
        assert!(portable_problems("Plan 2026 – Übersicht.md", TYPED_NAME_MAX).is_empty());
        assert!(portable_problems("  padded  ", TYPED_NAME_MAX).is_empty(), "outer spaces are trimmed, not a problem");
        assert_eq!(portable_problems("a:b", TYPED_NAME_MAX), vec!["`:` is not allowed on Windows"]);
        assert_eq!(portable_problems("a:b?c:d", TYPED_NAME_MAX), vec!["`:` `?` are not allowed on Windows"]);
        assert_eq!(portable_problems("tab\there", TYPED_NAME_MAX), vec!["control characters are not allowed on Windows"]);
        assert_eq!(portable_problems("end.", TYPED_NAME_MAX), vec!["a name can't end in a dot on Windows"]);
        assert_eq!(portable_problems("con.txt", TYPED_NAME_MAX), vec!["`con` is a reserved name on Windows"]);
        assert_eq!(portable_problems(&"x".repeat(201), TYPED_NAME_MAX), vec!["the name is longer than 200 bytes"]);
        assert!(portable_problems(&"x".repeat(200), TYPED_NAME_MAX).is_empty());
        assert_eq!(portable_problems(&"ü".repeat(101), TYPED_NAME_MAX).len(), 1, "bytes, not characters");
    }

    // spec: VAULT-027
    #[test]
    fn the_notice_says_what_a_typed_name_became_and_is_silent_when_nothing_changed() {
        assert_eq!(portable_notice("Saved as", "a:b", "a-b.md", TYPED_NAME_MAX).as_deref(), Some("Saved as `a-b.md`: `:` is not allowed on Windows"));
        assert_eq!(portable_notice("Renamed to", "Plan", "Plan.md", TYPED_NAME_MAX), None);
        // a path: every part is looked at, a reason is told once
        assert_eq!(portable_notice("Saved as", "x:y/z:w/CON", "CON_.md", TYPED_NAME_MAX).as_deref(), Some("Saved as `CON_.md`: `:` is not allowed on Windows; `CON` is a reserved name on Windows"));
        assert_eq!(portable_notice("Saved as", &"n".repeat(300), "n.md", TYPED_NAME_MAX).as_deref(), Some("Saved as `n.md`: the name is longer than 200 bytes"));
    }
}
