//! Opening things with another program (browser, mail program, document viewer).
//!
//! This is the only place that starts `xdg-open` (Linux), `open` (macOS) or `ShellExecuteW`
//! (Windows, with no shell: never `cmd /C start`, SEC-022). What it is given can come
//! from a remote server (calendar event links) or from a mail (attachments), so it checks first:
//!
//! - web addresses: only `http`, `https` and `mailto`; `www.…` becomes `https://www.…`;
//! - files: only files inside the vault, copies Shale made in its own private temp folder, and
//!   a PDF Shale just exported; never programs, scripts or `.desktop` files, and never a
//!   file with the executable bit (except known document types, since some mounted disks mark
//!   every file executable).
//!
//! Every program is started with an argument list, never through a shell.

use std::path::{Path, PathBuf};

/// Open a web or mail address in the default program. Returns why not if it is not allowed.
pub fn open_url(url: &str) -> Result<(), String> {
    let url = check_url(url)?;
    launch(url.as_ref())
}

/// Open an OAuth2 sign-in page in the browser. Only `https://` addresses; `http://` only for an
/// address on this computer and only when `allow_local_http` (the Custom provider, MAIL-082).
/// In tests the address goes to the installed "browser" (`set_test_browser`) instead.
pub fn open_signin_url(url: &str, allow_local_http: bool) -> Result<(), String> {
    let url = url.trim();
    let origin = crate::calendar::dav::Origin::parse(url)?;
    #[cfg(test)]
    let allow_local_http = allow_local_http || (origin.is_local() && *trusted_local_port().lock().unwrap_or_else(|e| e.into_inner()) == Some(origin.port));
    if origin.scheme != "https" && !(allow_local_http && origin.is_local()) {
        return Err("The sign-in address must start with https://".into());
    }
    #[cfg(test)]
    if let Some(b) = test_browser().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        b(url);
        return Ok(());
    }
    open_url(url)
}

/// A pretend browser for tests: gets the sign-in address instead of a real browser.
#[cfg(test)]
pub type TestBrowser = Box<dyn Fn(&str) + Send + Sync>;

#[cfg(test)]
fn test_browser() -> &'static std::sync::Mutex<Option<TestBrowser>> {
    static B: std::sync::Mutex<Option<TestBrowser>> = std::sync::Mutex::new(None);
    &B
}

/// The one local port whose `http://` sign-in addresses the pretend browser accepts (a pretend
/// Mastodon server on this computer): set by [`install_test_browser_for_local_server`].
#[cfg(test)]
fn trusted_local_port() -> &'static std::sync::Mutex<Option<u16>> {
    static P: std::sync::Mutex<Option<u16>> = std::sync::Mutex::new(None);
    &P
}

/// Holds the pretend browser; the next test that wants one waits for this one to be dropped.
#[cfg(test)]
pub struct BrowserGuard {
    _serial: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for BrowserGuard {
    fn drop(&mut self) {
        *test_browser().lock().unwrap_or_else(|e| e.into_inner()) = None;
        *trusted_local_port().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Tests only: install the pretend browser used by [`open_signin_url`] until the guard is dropped.
#[cfg(test)]
pub fn install_test_browser(b: TestBrowser) -> BrowserGuard {
    let g = no_test_browser();
    *test_browser().lock().unwrap_or_else(|e| e.into_inner()) = Some(b);
    g
}

/// Tests only: wait until no other test has the pretend browser, and keep it away until the
/// guard is dropped (a test of what happens without one).
#[cfg(test)]
pub fn no_test_browser() -> BrowserGuard {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    BrowserGuard { _serial: SERIAL.lock().unwrap_or_else(|e| e.into_inner()) }
}

/// Tests only: like [`install_test_browser`], and sign-in addresses on `http://127.0.0.1:<port>`
/// of one pretend server on this computer (a Mastodon server of a test) are accepted although the
/// Feeds sign-in is `https` only. Other ports and other hosts are still refused.
#[cfg(test)]
pub fn install_test_browser_for_local_server(port: u16, b: TestBrowser) -> BrowserGuard {
    let g = install_test_browser(b);
    *trusted_local_port().lock().unwrap_or_else(|e| e.into_inner()) = Some(port);
    g
}

/// Open a file of the vault with its default program.
pub fn open_vault_file(path: &Path, vault_root: &Path) -> Result<(), String> {
    launch(check_vault_file(path, vault_root)?.as_ref())
}

/// Open a media file outside the vault with its default program (MEDIA-017): a picture, PDF, sound
/// or video that a library link or a file link names. Nothing else is opened from there.
pub fn open_outside_media(path: &Path) -> Result<(), String> {
    launch(check_outside_media(path)?.as_ref())
}

/// Open the PDF that was just exported (it may be outside the vault, where the user chose).
pub fn open_exported_pdf(path: &Path) -> Result<(), String> {
    launch(check_exported_pdf(path)?.as_ref())
}

/// Open the Word file that was just exported (it may be outside the vault, where the user chose).
pub fn open_exported_docx(path: &Path) -> Result<(), String> {
    launch(check_exported_docx(path)?.as_ref())
}

/// Open a folder of rescued notes (see `recovery`) in the file manager. Only folders inside the
/// recovery folder are opened.
pub fn open_recovery_folder(path: &Path, recovery_root: &Path) -> Result<(), String> {
    if !crate::recovery::is_inside(recovery_root, path) {
        return Err("Not opened: not a folder of rescued notes".into());
    }
    launch(path.as_os_str())
}

/// Open the themes folder in the file manager; it is made first when it is not there yet.
pub fn open_themes_folder(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not make the themes folder: {e}"))?;
    launch(dir.as_os_str())
}

/// Save `data` (a mail attachment, say) as `name` in Shale's private temp folder and open that
/// copy. Each call uses a fresh folder, so same-named attachments never clash.
pub fn open_temp_copy(name: &str, data: &[u8]) -> Result<(), String> {
    let path = save_temp_copy(name, data)?;
    let checked = match check_temp_file(&path) {
        Ok(p) => p,
        Err(e) => {
            // a refused copy (a program, a .desktop file) is not left lying around
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Err(e);
        }
    };
    launch(checked.as_ref())
}

/// The address to open, or why it is refused. Control characters (which could smuggle in a
/// second line or option) and unknown schemes are refused.
pub fn check_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("There is no address to open".into());
    }
    if url.chars().any(|c| c.is_control()) {
        return Err("Not opened: the address contains control characters".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("www.") {
        return Ok(format!("https://{url}"));
    }
    let scheme = lower.split_once(':').map(|(s, _)| s).unwrap_or("");
    match scheme {
        "http" | "https" | "mailto" => Ok(url.to_string()),
        _ => Err(format!("Not opened: only web and mail addresses are opened, not “{}”", url.chars().take(40).collect::<String>())),
    }
}

/// File types that start a program or script. Never opened, whoever offers them.
const PROGRAM_EXTENSIONS: &[&str] = &[
    "desktop",
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "fish",
    "exe",
    "bat",
    "cmd",
    "com",
    "appimage",
    "jar",
    "py",
    "pyc",
    "pl",
    "pm",
    "rb",
    "run",
    "bin",
    "msi",
    "scr",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "cpl",
    "lnk",
    "reg",
    "app",
    "command",
    "workflow",
    "action",
    "pkg",
    "dmg",
    "deb",
    "rpm",
    "php",
    "jnlp",
    "elf",
    "out",
    "so",
    "dll",
    "url",
    "webloc",
    "terminal",
    "service",
    "flatpakref",
    "snap",
    // more Windows program and script types (the list is the same on every system, SEC-022)
    "pif",
    "msp",
    "msc",
    "scf",
    "inf",
    "chm",
    "gadget",
    "application",
    "appref-ms",
    "vb",
    "ws",
];

/// Document types that are safe to open even if the file system marks them executable (where
/// there is an executable bit at all).
#[cfg(unix)]
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "txt", "md", "csv", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "mp3", "mp4", "m4v", "m4a", "wav", "ogg", "flac", "mkv", "webm", "mov", "avi",
    "eml", "ics", "vcf", "zip", "epub",
];

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Why this file must not be handed to another program, if so: a program type, or marked
/// executable (documents excepted).
fn refuse_program(path: &Path) -> Result<(), String> {
    let ext = extension(path);
    if PROGRAM_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Not opened: “.{ext}” files can run programs"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = std::fs::metadata(path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
        if exec && !DOCUMENT_EXTENSIONS.contains(&ext.as_str()) {
            return Err("Not opened: the file is executable".into());
        }
    }
    Ok(())
}

/// A regular file (not a folder, not a device), resolved through links.
fn real_file(path: &Path) -> Result<PathBuf, String> {
    let real = std::fs::canonicalize(path).map_err(|e| format!("Not opened: {}: {e}", path.display()))?;
    if !real.is_file() {
        return Err("Not opened: not a regular file".into());
    }
    Ok(real)
}

/// The file to open if it lies inside the vault (after resolving links, so a link in the vault
/// pointing outside is refused) and is not a program.
pub fn check_vault_file(path: &Path, vault_root: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    let root = std::fs::canonicalize(vault_root).map_err(|e| format!("Not opened: {e}"))?;
    if !real.starts_with(&root) {
        return Err("Not opened: the file is outside the vault".into());
    }
    refuse_program(path)?;
    refuse_program(&real)?;
    Ok(real)
}

/// The file to open if it is a media file (picture, PDF, sound or video by its name, and not a
/// program), a regular file, after resolving links: the files of outside links are opened with no
/// vault around them, so the kind of file is the check (MEDIA-017).
pub fn check_outside_media(path: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    if crate::media::media_kind(path).is_none() || crate::media::media_kind(&real).is_none() {
        return Err("Not opened: only pictures, PDFs, sounds and videos are opened from outside the vault".into());
    }
    refuse_program(path)?;
    refuse_program(&real)?;
    Ok(real)
}

/// The exported PDF to open: a PDF file that is not executable.
pub fn check_exported_pdf(path: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    if extension(&real) != "pdf" {
        return Err("Not opened: not a PDF".into());
    }
    refuse_program(&real)?;
    Ok(real)
}

/// The file to open if it is a Word file that exists and not a program.
fn check_exported_docx(path: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    if extension(&real) != "docx" {
        return Err("Not opened: not a Word file".into());
    }
    refuse_program(&real)?;
    Ok(real)
}

/// The file to open if it is inside Shale's private temp folder and not a program.
fn check_temp_file(path: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    let root = std::fs::canonicalize(temp_root()?).map_err(|e| format!("Not opened: {e}"))?;
    if !real.starts_with(&root) {
        return Err("Not opened: the file is not in Shale's temp folder".into());
    }
    refuse_program(&real)?;
    Ok(real)
}

/// Shale's own temp folder, owner-only (0700) and owned by the current user. It is in the
/// user's runtime folder when there is one (already private), else in the system temp folder,
/// where an existing folder of that name that is not ours is refused.
fn temp_root() -> Result<PathBuf, String> {
    crate::fsutil::private_temp_root("shale-open").map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { "Not opened: the temp folder is not ours".to_string() } else { format!("Could not make the temp folder: {e}") })
}

/// A file name that is safe to create: no folders in it, nothing hidden, not empty.
fn safe_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | '\0') { '_' } else { c }).collect();
    let clean = clean.trim().trim_start_matches('.').chars().take(120).collect::<String>();
    if clean.is_empty() {
        "file".into()
    } else {
        clean
    }
}

fn save_temp_copy(name: &str, data: &[u8]) -> Result<PathBuf, String> {
    let root = temp_root()?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = root.join(format!("{}-{nanos:x}", std::process::id()));
    crate::fsutil::ensure_private_dir(&dir).map_err(|e| format!("Could not make the temp folder: {e}"))?;
    let path = dir.join(safe_name(name));
    crate::fsutil::create_new(&path, data).map_err(|e| format!("Could not save the file: {e}"))?;
    Ok(path)
}

/// A Windows path without the verbatim prefix that `canonicalize` adds (`\\?\C:\x` becomes
/// `C:\x`, `\\?\UNC\host\share` becomes `\\host\share`): the shell does not understand it.
/// Anything else is returned unchanged. Works on UTF-16 text, so no path is ever altered by a
/// conversion.
#[cfg(any(windows, test))]
fn strip_verbatim(path: &[u16]) -> Vec<u16> {
    let wide = |t: &str| t.encode_utf16().collect::<Vec<u16>>();
    let unc = wide(r"\\?\UNC\");
    if path.starts_with(&unc) {
        return [wide(r"\\"), path[unc.len()..].to_vec()].concat();
    }
    let verbatim = wide(r"\\?\");
    // only the drive form: other verbatim names (devices, volume GUIDs) are left alone
    let drive = path.len() >= verbatim.len() + 2 && path[verbatim.len() + 1] == u16::from(b':') && char::from_u32(u32::from(path[verbatim.len()])).is_some_and(|c| c.is_ascii_alphabetic());
    if path.starts_with(&verbatim) && drive {
        return path[verbatim.len()..].to_vec();
    }
    path.to_vec()
}

/// What the Windows shell is asked: the verb `open` and the file or address, both as
/// NUL-terminated UTF-16 text, the verbatim prefix removed (SEC-022). Everything but the call
/// itself, so tests on every system can check it.
#[cfg(any(windows, test))]
fn shell_request(target: &[u16]) -> (Vec<u16>, Vec<u16>) {
    let file = strip_verbatim(target).into_iter().chain(std::iter::once(0)).collect();
    let verb = "open".encode_utf16().chain(std::iter::once(0)).collect();
    (verb, file)
}

/// Windows: hand the address or file to the shell's "open" verb. No command line is built, so
/// nothing in `target` can be taken for a switch or a second command (SEC-005).
#[cfg(windows)]
fn launch_windows(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let raw: Vec<u16> = target.encode_wide().collect();
    let (verb, file) = shell_request(&raw);
    // tests: a recording fake on this thread only (`FakeXdgOpen`) gets the verb and file instead of the shell
    #[cfg(test)]
    if record_shell_open(&verb, &file) {
        return Ok(());
    }
    // a thread of its own: the shell wants COM set up on the calling thread, and it may take a moment
    let code = std::thread::spawn(move || {
        // SAFETY: the strings are NUL-terminated and outlive the call; the other arguments are null.
        unsafe {
            let hr = CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as _);
            let r = ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) as usize;
            if hr >= 0 {
                CoUninitialize();
            }
            r
        }
    })
    .join()
    .map_err(|_| "Could not open it: the shell call failed".to_string())?;
    // ShellExecuteW answers with a value above 32 when it worked, an error code up to 32 if not
    if code > 32 {
        Ok(())
    } else {
        Err(format!("Could not open it: the system answered {code}"))
    }
}

/// Start the system's opener for an address or a file that has been checked.
fn launch(target: &std::ffi::OsStr) -> Result<(), String> {
    // tests: the pretend program gets what would be opened (nothing starts). A recording fake
    // `xdg-open` of this thread (`FakeXdgOpen`) comes first: it tests the real start-up.
    #[cfg(test)]
    if !fake_opener_installed() {
        if let Some(b) = test_browser().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            b(&target.to_string_lossy());
            return Ok(());
        }
        // a test build never starts the real opener (#313): without a fake the call is only noted
        unfaked_opens().lock().unwrap_or_else(|e| e.into_inner()).push(target.to_string_lossy().into_owned());
        return Ok(());
    }
    #[cfg(windows)]
    {
        launch_windows(target)
    }
    #[cfg(not(windows))]
    {
        launch_program(target)
    }
}

/// Linux and macOS: the opener program, started with the target as its only argument.
#[cfg(not(windows))]
fn launch_program(target: &std::ffi::OsStr) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        #[cfg(target_os = "linux")]
        let program = "xdg-open";
        #[cfg(target_os = "macos")]
        let program = "open";
        let mut command = crate::program::command(program);
        // tests: a recording fake on this thread only (`FakeXdgOpen`), so parallel tests never see it
        #[cfg(test)]
        if let Some(fake) = fake_opener_command() {
            command = fake;
        }
        let mut child = command.arg(target).spawn().map_err(|e| format!("Could not open it: {e}"))?;
        // reap it in the background so it never stays a zombie
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = target;
        Err("Could not open it: unsupported".to_string())
    }
}

#[cfg(all(test, unix))]
thread_local! {
    /// The script of the fake opener of this test thread.
    static FAKE_OPENER: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, windows))]
thread_local! {
    /// The calls (one file each) the fake shell of this test thread has got.
    static FAKE_OPENER: std::cell::RefCell<Option<Vec<Vec<String>>>> = const { std::cell::RefCell::new(None) };
}

/// Tests: what was opened with no fake and no pretend browser installed; nothing was started (#313).
#[cfg(test)]
fn unfaked_opens() -> &'static std::sync::Mutex<Vec<String>> {
    static OPENS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    &OPENS
}

#[cfg(test)]
fn fake_opener_installed() -> bool {
    FAKE_OPENER.with(|f| f.borrow().is_some())
}

#[cfg(all(test, not(windows)))]
fn fake_opener_command() -> Option<std::process::Command> {
    // run through `sh`, never by executing the written file (a parallel test's fork could hold
    // it open for writing, which makes starting it fail with "text file busy")
    FAKE_OPENER.with(|f| {
        f.borrow().as_ref().map(|script| {
            let mut c = std::process::Command::new("sh");
            c.arg(script).arg("--");
            c
        })
    })
}

/// Windows tests: what the shell would have got (verb and file, NUL-terminated) goes to the
/// fake of this thread, if there is one. Answers whether it did.
#[cfg(all(test, windows))]
fn record_shell_open(verb: &[u16], file: &[u16]) -> bool {
    let text = |w: &[u16]| String::from_utf16_lossy(w.strip_suffix(&[0]).unwrap_or(w));
    assert_eq!(text(verb), "open", "the shell is always asked for the open verb");
    FAKE_OPENER.with(|f| match f.borrow_mut().as_mut() {
        Some(calls) => {
            calls.push(vec![text(file)]);
            true
        }
        None => false,
    })
}

/// Tests only: a recording fake opener for the calling thread, whatever the system. On Linux
/// and macOS (`xdg-open` / `open`) everything the opener starts on this thread goes to a small
/// shell script that writes the arguments it got to a file; the real chokepoint (checks, temp
/// copies, the argument list, starting a child) is exercised. It needs a Unix shell. On Windows
/// the fake takes the place of the `ShellExecuteW` call only: it records the file the shell
/// would have been given (verbatim prefix already removed), after the verb was checked to be
/// `open`.
#[cfg(test)]
pub struct FakeXdgOpen {
    #[cfg(unix)]
    dir: PathBuf,
}

#[cfg(test)]
impl FakeXdgOpen {
    #[cfg(unix)]
    pub fn install() -> FakeXdgOpen {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("shale-fake-xdg-open-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("calls")).unwrap();
        // one file per call (written whole, then renamed), one argument per line
        let script = dir.join("xdg-open");
        let text = format!("#!/bin/sh\nshift\nf=\"{d}/calls/$$.tmp\"\nprintf '%s\\n' \"$@\" > \"$f\" && mv \"$f\" \"{d}/calls/$$.call\"\n", d = dir.display());
        std::fs::write(&script, text).unwrap();
        FAKE_OPENER.with(|f| *f.borrow_mut() = Some(script));
        FakeXdgOpen { dir }
    }

    #[cfg(windows)]
    pub fn install() -> FakeXdgOpen {
        FAKE_OPENER.with(|f| *f.borrow_mut() = Some(Vec::new()));
        FakeXdgOpen {}
    }

    /// Wait until `n` calls have come in (at most 10 s) and return the argument lists, oldest
    /// first. Fails the test when there are fewer, or more.
    pub fn calls(&self, n: usize) -> Vec<Vec<String>> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let got = self.recorded();
            if got.len() >= n || std::time::Instant::now() > deadline {
                assert_eq!(got.len(), n, "the fake xdg-open got these calls: {got:?}");
                return got;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The calls so far (nothing is waited for): one argument list each, oldest first.
    #[cfg(unix)]
    pub fn recorded(&self) -> Vec<Vec<String>> {
        let mut v: Vec<_> = std::fs::read_dir(self.dir.join("calls"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "call"))
            .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap(), std::fs::read_to_string(e.path()).unwrap().lines().map(String::from).collect::<Vec<_>>()))
            .collect();
        v.sort_by_key(|(t, _)| *t);
        v.into_iter().map(|(_, a)| a).collect()
    }

    #[cfg(windows)]
    pub fn recorded(&self) -> Vec<Vec<String>> {
        FAKE_OPENER.with(|f| f.borrow().clone().unwrap_or_default())
    }
}

#[cfg(test)]
impl Drop for FakeXdgOpen {
    fn drop(&mut self) {
        FAKE_OPENER.with(|f| *f.borrow_mut() = None);
        #[cfg(unix)]
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-opener-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: SEC-006
    #[test]
    fn only_web_and_mail_addresses_are_opened() {
        for ok in ["http://example.com", "https://example.com/a?b=c#d", "HTTPS://Example.com", "HtTp://x.y", "mailto:ada@example.com", "MAILTO:a@b.c", "  https://example.com  "] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        assert_eq!(check_url("www.example.com").unwrap(), "https://www.example.com");
        assert_eq!(check_url("WWW.Example.com").unwrap(), "https://WWW.Example.com");
        for bad in [
            "",
            "   ",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com",
            "data:text/html,hi",
            "smb://host/share",
            "ssh://host",
            "tel:123",
            "vscode://open",
            "/etc/passwd",
            "example.com",
            "-n",
            "--help",
            "https://example.com\nfile:///etc/passwd",
            "https://exa\0mple.com",
            "http://x\r\ny",
            "https://a\u{7f}b",
            "ht tp://x",
            "https//x",
            "x-scheme:http://a",
        ] {
            assert!(check_url(bad).is_err(), "{bad:?} must be refused");
        }
    }

    // spec: SEC-006
    #[test]
    fn vault_files_are_checked() {
        let root = tmp("vault");
        let out = tmp("outside");
        fs::write(root.join("doc.pdf"), "x").unwrap();
        fs::write(root.join("Photo.PNG"), "x").unwrap();
        fs::write(out.join("secret.txt"), "x").unwrap();
        assert!(check_vault_file(&root.join("doc.pdf"), &root).is_ok());
        assert!(check_vault_file(&root.join("Photo.PNG"), &root).is_ok());
        // outside the vault, also through a ../ path
        assert!(check_vault_file(&out.join("secret.txt"), &root).is_err());
        let sneaky = root.join("..").join(out.file_name().unwrap()).join("secret.txt");
        assert!(check_vault_file(&sneaky, &root).is_err());
        // missing files and folders
        assert!(check_vault_file(&root.join("nope.pdf"), &root).is_err());
        assert!(check_vault_file(&root, &root).is_err());
        // a link inside the vault that points outside
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(out.join("secret.txt"), root.join("link.txt")).unwrap();
            assert!(check_vault_file(&root.join("link.txt"), &root).is_err());
        }
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&out);
    }

    // spec: SEC-006
    #[test]
    fn programs_and_desktop_files_are_never_opened() {
        let root = tmp("programs");
        for name in [
            "a.desktop",
            "run.sh",
            "setup.EXE",
            "x.bat",
            "x.cmd",
            "x.com",
            "app.AppImage",
            "t.jar",
            "s.py",
            "s.pl",
            "s.rb",
            "i.run",
            "d.bin",
            "m.msi",
            "p.ps1",
            "v.vbs",
            "j.js",
            "l.lnk",
            "Thing.app",
            "a.command",
            "x.deb",
            "x.rpm",
            "x.service",
        ] {
            fs::write(root.join(name), "x").unwrap();
            assert!(check_vault_file(&root.join(name), &root).is_err(), "{name} must be refused");
        }
        // a harmless looking name on a link to a program is refused as well
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("run.sh"), root.join("innocent.txt")).unwrap();
            assert!(check_vault_file(&root.join("innocent.txt"), &root).is_err());
        }
        let _ = fs::remove_dir_all(&root);
    }

    // spec: SEC-006
    #[cfg(unix)]
    #[test]
    fn executable_files_are_refused_but_documents_are_not() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp("exec");
        for (name, ok) in [("tool", false), ("noext.", false), ("data.xyz", false), ("report.pdf", true), ("pic.jpg", true)] {
            let p = root.join(name);
            fs::write(&p, "x").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(check_vault_file(&p, &root).is_ok(), ok, "{name}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    // spec: DOCX-002
    #[test]
    fn an_exported_word_file_is_a_docx() {
        let d = tmp("export-docx");
        fs::write(d.join("a.docx"), "x").unwrap();
        fs::write(d.join("a.pdf"), "x").unwrap();
        fs::write(d.join("b.sh"), "x").unwrap();
        assert!(check_exported_docx(&d.join("a.docx")).is_ok());
        assert!(check_exported_docx(&d.join("a.pdf")).is_err());
        assert!(check_exported_docx(&d.join("b.sh")).is_err());
        assert!(check_exported_docx(&d.join("missing.docx")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    // spec: SEC-006
    #[test]
    fn an_exported_pdf_is_a_pdf() {
        let d = tmp("export");
        fs::write(d.join("a.pdf"), "x").unwrap();
        fs::write(d.join("a.txt"), "x").unwrap();
        fs::write(d.join("b.sh"), "x").unwrap();
        assert!(check_exported_pdf(&d.join("a.pdf")).is_ok());
        assert!(check_exported_pdf(&d.join("a.txt")).is_err());
        assert!(check_exported_pdf(&d.join("b.sh")).is_err());
        assert!(check_exported_pdf(&d.join("missing.pdf")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    // spec: SEC-006, MEDIA-017
    #[test]
    fn a_file_outside_the_vault_is_opened_only_when_it_is_media() {
        let d = tmp("outside-media");
        for name in ["a.mp4", "A.PDF", "b.png", "c.flac", "notes.txt", "d.docx", "run.sh", "e.desktop", "f.exe"] {
            fs::write(d.join(name), "x").unwrap();
        }
        for ok in ["a.mp4", "A.PDF", "b.png", "c.flac"] {
            assert!(check_outside_media(&d.join(ok)).is_ok(), "{ok}");
        }
        for bad in ["notes.txt", "d.docx", "run.sh", "e.desktop", "f.exe", "missing.mp4"] {
            assert!(check_outside_media(&d.join(bad)).is_err(), "{bad}");
        }
        assert!(check_outside_media(&d).is_err(), "a folder");
        #[cfg(unix)]
        {
            // a media name that is a link to something else, and an executable one
            std::os::unix::fs::symlink(d.join("run.sh"), d.join("link.mp4")).unwrap();
            assert!(check_outside_media(&d.join("link.mp4")).is_err(), "the link is judged by what it points to");
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(d.join("a.mp4"), fs::Permissions::from_mode(0o755)).unwrap();
            assert!(check_outside_media(&d.join("a.mp4")).is_ok(), "a document type may carry the executable bit");
        }
        let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = opened.clone();
        let _guard = install_test_browser(Box::new(move |t| sink.lock().unwrap().push(t.to_string())));
        assert!(open_outside_media(&d.join("a.mp4")).is_ok());
        assert!(open_outside_media(&d.join("run.sh")).is_err());
        let opened = opened.lock().unwrap();
        assert_eq!(opened.len(), 1, "only the media file was handed over: {opened:?}");
        assert!(opened[0].ends_with("a.mp4"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: SEC-006
    #[test]
    fn temp_copies_live_in_a_private_folder_and_are_checked() {
        let p = save_temp_copy("../../evil name.pdf", b"%PDF").unwrap();
        assert!(check_temp_file(&p).is_ok());
        assert_eq!(fs::read(&p).unwrap(), b"%PDF");
        assert!(p.file_name().unwrap().to_string_lossy().ends_with("evil name.pdf"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(temp_root().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        // same name again: a new folder, nothing overwritten
        let q = save_temp_copy("../../evil name.pdf", b"other").unwrap();
        assert_ne!(p, q);
        assert_eq!(fs::read(&p).unwrap(), b"%PDF");
        // programs are refused even in the temp folder, and files elsewhere are not "temp copies"
        let s = save_temp_copy("run.sh", b"#!/bin/sh").unwrap();
        assert!(check_temp_file(&s).is_err());
        let d = tmp("elsewhere");
        fs::write(d.join("a.pdf"), "x").unwrap();
        assert!(check_temp_file(&d.join("a.pdf")).is_err());
        let _ = fs::remove_dir_all(&d);
        for f in [&p, &q, &s] {
            let _ = fs::remove_dir_all(f.parent().unwrap());
        }
    }

    // spec: SEC-022
    #[test]
    fn the_verbatim_prefix_is_removed_before_a_path_goes_to_the_windows_shell() {
        let w = |t: &str| t.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(strip_verbatim(&w(r"\\?\C:\Users\me\a.pdf")), w(r"C:\Users\me\a.pdf"));
        assert_eq!(strip_verbatim(&w(r"\\?\UNC\host\share\a.pdf")), w(r"\\host\share\a.pdf"));
        // everything else stays as it is
        for same in [r"C:\Users\me\a.pdf", r"\\host\share\a.pdf", r"\\?\Volume{1234}\a.pdf", r"\\?\", r"\\?\C", "https://example.com/?a=1", ""] {
            assert_eq!(strip_verbatim(&w(same)), w(same), "{same}");
        }
    }

    // spec: SEC-022
    #[test]
    fn the_windows_shell_is_asked_for_the_open_verb_and_a_file_without_the_verbatim_prefix() {
        let w = |t: &str| t.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        for (given, file) in [(r"\\?\C:\Users\me\a b.pdf", r"C:\Users\me\a b.pdf"), (r"\\?\UNC\host\share\a.pdf", r"\\host\share\a.pdf"), (r"C:\a.pdf", r"C:\a.pdf"), ("https://example.com/a?b=c&d", "https://example.com/a?b=c&d")] {
            let raw: Vec<u16> = given.encode_utf16().collect();
            assert_eq!(shell_request(&raw), (w("open"), w(file)), "{given}");
        }
    }

    // spec: SEC-022
    #[test]
    fn program_types_of_every_system_are_refused_on_every_system() {
        let root = tmp("types");
        for name in ["a.exe", "a.BAT", "a.cmd", "a.ps1", "a.msi", "a.lnk", "a.hta", "a.app", "a.command", "a.desktop", "a.sh", "a.jar", "a.vbs", "a.scr", "a.pif", "a.chm"] {
            fs::write(root.join(name), "x").unwrap();
            assert!(check_vault_file(&root.join(name), &root).is_err(), "{name}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn temp_names_are_made_safe() {
        assert_eq!(safe_name("a/b\\c.txt"), "a_b_c.txt");
        assert_eq!(safe_name(".hidden"), "hidden");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("a\nb"), "a_b");
        assert!(safe_name(&"x".repeat(500)).len() <= 120);
    }

    // spec: SEC-006, SEC-005
    #[test]
    fn no_other_code_starts_an_opener() {
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
                if !name.ends_with(".rs") || name == "opener.rs" {
                    continue;
                }
                let text = fs::read_to_string(&p).unwrap();
                let code = text.split("#[cfg(test)]").next().unwrap_or("");
                for (i, line) in code.lines().enumerate() {
                    let l = line.trim_start();
                    if !l.starts_with("//") && (l.contains("\"xdg-open\"") || l.contains("Command::new(\"open\")") || l.contains("command(\"open\")") || l.contains("\"/C\"") || l.contains("ShellExecute")) {
                        bad.push(format!("{name}:{}: {l}", i + 1));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "open things through opener instead:\n{}", bad.join("\n"));
    }
}

/// SEC-006 end to end: the real opener starts a (fake, recording) `xdg-open` (on Windows: asks a
/// recording fake shell), so the checks, the argument list and the start of the child are all
/// exercised, not only `check_url` and friends. SEC-022: runs on every system.
#[cfg(test)]
mod real_opener_tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    /// What the opener hands over for a real path: on Windows the shell gets it without the
    /// verbatim prefix that `canonicalize` adds, elsewhere as it is.
    fn handed(path: &Path) -> PathBuf {
        let wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
        PathBuf::from(String::from_utf16_lossy(&strip_verbatim(&wide)))
    }

    /// A fresh folder of this test (counter, not only the process id).
    fn folder(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("shale-opener-real-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: SEC-022
    #[test]
    fn a_test_build_without_a_fake_starts_no_opener_and_answers_ok() {
        assert!(!fake_opener_installed());
        let _no_browser = no_test_browser();
        let name = format!("shale-no-opener-{}.pdf", std::process::id());
        // with no fake the real `xdg-open` would start, open a viewer and fail once the copy is gone
        assert_eq!(open_temp_copy(&name, b"%PDF-1.4 x"), Ok(()));
        let noted = unfaked_opens().lock().unwrap().clone();
        let path = noted.iter().find(|p| p.ends_with(&name)).unwrap_or_else(|| panic!("the call is only noted: {noted:?}"));
        let _ = fs::remove_dir_all(Path::new(path).parent().unwrap());
        // the fake of the thread still gets the real start-up
        let fake = FakeXdgOpen::install();
        open_url("https://example.com/x").unwrap();
        assert_eq!(fake.calls(1), [vec!["https://example.com/x".to_string()]]);
    }

    // spec: SEC-006
    #[test]
    fn the_fake_xdg_open_gets_exactly_the_allowed_web_and_mail_addresses_as_one_argument() {
        let fake = FakeXdgOpen::install();
        let mut want = vec![];
        for (given, passed) in [
            ("https://example.com/a?b=c&d=e#f", "https://example.com/a?b=c&d=e#f"),
            ("http://example.com", "http://example.com"),
            ("HTTPS://Example.com/X", "HTTPS://Example.com/X"),
            ("mailto:ada@example.com?subject=Hi%20there", "mailto:ada@example.com?subject=Hi%20there"),
            ("  https://example.com/trimmed  ", "https://example.com/trimmed"),
            ("www.example.com", "https://www.example.com"),
            ("https://example.com/a b;rm -rf $HOME `x`", "https://example.com/a b;rm -rf $HOME `x`"),
        ] {
            open_url(given).unwrap_or_else(|e| panic!("{given}: {e}"));
            want.push(vec![passed.to_string()]);
            assert_eq!(fake.calls(want.len()), want, "{given}");
        }
    }

    // spec: SEC-006
    #[test]
    fn the_fake_xdg_open_never_gets_other_schemes_or_addresses_with_control_characters() {
        let fake = FakeXdgOpen::install();
        for bad in [
            "file:///etc/passwd",
            "FILE:///etc/passwd",
            "javascript:alert(1)",
            // a scheme a calendar server may put into an event link
            "ftp://files.example.com/a.sh",
            "data:text/html,hi",
            "smb://host/share",
            "-n",
            "--help",
            "/etc/passwd",
            "https://example.com\nfile:///etc/passwd",
            "https://exa\0mple.com",
            "",
        ] {
            assert!(open_url(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(fake.recorded().is_empty(), "nothing was started: {:?}", fake.recorded());
        // the sign-in address has its own rule and the same chokepoint
        assert!(open_signin_url("http://example.com/auth", true).is_err());
        assert!(open_signin_url("file:///etc/passwd", true).is_err());
        assert!(fake.recorded().is_empty());
        // the opener still works afterwards, and what it got is only this one
        open_url("https://example.com/sentinel").unwrap();
        assert_eq!(fake.calls(1), vec![vec!["https://example.com/sentinel".to_string()]]);
    }

    // spec: SEC-006
    #[test]
    fn the_fake_xdg_open_gets_a_vault_file_by_its_real_path_and_never_a_file_outside_a_program_or_a_desktop_file() {
        let fake = FakeXdgOpen::install();
        let root = folder("vault");
        let outside = folder("outside");
        fs::write(root.join("my doc.pdf"), "x").unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        fs::write(root.join("run.sh"), "#!/bin/sh").unwrap();
        fs::write(root.join("evil.desktop"), "[Desktop Entry]\nExec=sh").unwrap();
        fs::write(root.join("scan.pdf"), "x").unwrap();
        let refused = vec![
            ("a file outside the vault", outside.join("secret.txt")),
            ("a path that climbs out of the vault", root.join("..").join(outside.file_name().unwrap()).join("secret.txt")),
            ("a script", root.join("run.sh")),
            ("a .desktop file", root.join("evil.desktop")),
            ("a folder", root.clone()),
            ("a missing file", root.join("missing.pdf")),
        ];
        // Unix only: the executable bit and links. The vault is then reached through a link: the argument is the real path
        #[cfg(unix)]
        let (via, unix_only) = {
            fs::write(root.join("tool"), "x").unwrap();
            fs::set_permissions(root.join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(root.join("scan.pdf"), fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
            let via = folder("via-link").join("v");
            std::os::unix::fs::symlink(&root, &via).unwrap();
            (via, vec![("a link out of the vault", root.join("link.txt")), ("an executable file", root.join("tool"))])
        };
        #[cfg(not(unix))]
        let (via, unix_only): (PathBuf, Vec<(&str, PathBuf)>) = (root.clone(), vec![]);

        open_vault_file(&via.join("my doc.pdf"), &via).unwrap();
        let real = handed(&fs::canonicalize(root.join("my doc.pdf")).unwrap());
        assert_eq!(fake.calls(1), vec![vec![real.to_string_lossy().into_owned()]], "one argument, with the space in it");
        // a document that a mounted disk marks executable is still a document
        open_vault_file(&root.join("scan.pdf"), &root).unwrap();
        assert_eq!(fake.calls(2).len(), 2);

        for (what, path) in refused.into_iter().chain(unix_only) {
            assert!(open_vault_file(&path, &root).is_err(), "{what} must be refused");
        }
        assert_eq!(fake.recorded().len(), 2, "the refused files never reached xdg-open: {:?}", fake.recorded());
        for d in [&root, &outside] {
            let _ = fs::remove_dir_all(d);
        }
        #[cfg(unix)]
        let _ = fs::remove_dir_all(via.parent().unwrap());
    }

    // spec: SEC-006, MAIL-054
    #[test]
    fn a_mail_attachment_is_opened_as_a_private_temp_copy_and_programs_are_refused() {
        let fake = FakeXdgOpen::install();
        open_temp_copy("../../report 1.pdf", b"%PDF-1.4 hello").unwrap();
        let calls = fake.calls(1);
        let path = PathBuf::from(&calls[0][0]);
        assert_eq!(calls[0].len(), 1);
        assert_eq!(fs::read(&path).unwrap(), b"%PDF-1.4 hello", "the copy has the attachment's bytes");
        assert_eq!(path.file_name().unwrap(), "_.._report 1.pdf", "folders in the name are made harmless");
        assert!(path.starts_with(handed(&fs::canonicalize(temp_root().unwrap()).unwrap())), "{path:?} is in Shale's temp folder");
        #[cfg(unix)]
        assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        // the same name again: a copy of its own
        open_temp_copy("report 1.pdf", b"other").unwrap();
        let calls = fake.calls(2);
        assert_ne!(calls[0][0], calls[1][0]);
        assert_eq!(fs::read(&calls[0][0]).unwrap(), b"%PDF-1.4 hello", "the first copy is untouched");
        // programs and .desktop files are refused, whatever the mail says they are
        for name in ["setup.EXE", "run.sh", "evil.desktop", "x.AppImage", "s.py", "l.lnk"] {
            assert!(open_temp_copy(name, b"x").is_err(), "{name} must be refused");
        }
        assert_eq!(fake.recorded().len(), 2, "{:?}", fake.recorded());
        // ... and their copies are not left in the temp folder
        // (the temp folder is shared by every process of the user, so the name is this run's own:
        // another test process's refused copy, caught between written and removed, must not count)
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let own = format!("only-in-this-test-{}-{nanos:x}.desktop", std::process::id());
        assert!(open_temp_copy(&own, b"x").is_err());
        let left = fs::read_dir(temp_root().unwrap()).unwrap().flatten().filter(|e| e.path().join(&own).exists()).count();
        assert_eq!(left, 0, "a refused copy was removed");
        for c in &calls {
            let _ = fs::remove_dir_all(PathBuf::from(&c[0]).parent().unwrap());
        }
    }

    // spec: SEC-006
    #[test]
    fn the_exported_pdf_and_a_recovery_folder_go_to_the_fake_xdg_open_only_when_they_qualify() {
        let fake = FakeXdgOpen::install();
        let d = folder("export");
        let recovery = folder("recovery");
        let rescued = recovery.join("2024-05-01 rescue");
        fs::create_dir_all(&rescued).unwrap();
        fs::write(d.join("a.pdf"), "x").unwrap();
        fs::write(d.join("a.txt"), "x").unwrap();
        open_exported_pdf(&d.join("a.pdf")).unwrap();
        assert_eq!(fake.calls(1), vec![vec![handed(&fs::canonicalize(d.join("a.pdf")).unwrap()).to_string_lossy().into_owned()]]);
        assert!(open_exported_pdf(&d.join("a.txt")).is_err());
        open_recovery_folder(&rescued, &recovery).unwrap();
        assert!(open_recovery_folder(&d, &recovery).is_err(), "a folder outside the recovery folder");
        assert!(open_recovery_folder(&recovery.join("missing"), &recovery).is_err());
        let calls = fake.calls(2);
        assert_eq!(calls[1], vec![rescued.to_string_lossy().into_owned()]);
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(&recovery);
    }

    // spec: SEC-006
    #[test]
    fn a_fake_xdg_open_belongs_to_its_test_thread() {
        let fake = FakeXdgOpen::install();
        // another thread has none: its opener is the installed pretend browser or the real one, never this fake
        let other = std::thread::spawn(fake_opener_installed).join().unwrap();
        assert!(!other);
        open_url("https://example.com/mine").unwrap();
        assert_eq!(fake.calls(1).len(), 1);
        drop(fake);
        assert!(!fake_opener_installed(), "dropping it removes it");
    }
}
