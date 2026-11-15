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
    let origin = crate::cal_dav::Origin::parse(url)?;
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

/// Holds the pretend browser; the next test that wants one waits for this one to be dropped.
#[cfg(test)]
pub struct BrowserGuard {
    _serial: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for BrowserGuard {
    fn drop(&mut self) {
        *test_browser().lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Tests only: install the pretend browser used by [`open_signin_url`] until the guard is dropped.
#[cfg(test)]
pub fn install_test_browser(b: TestBrowser) -> BrowserGuard {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    *test_browser().lock().unwrap_or_else(|e| e.into_inner()) = Some(b);
    BrowserGuard { _serial: g }
}

/// Open a file of the vault with its default program.
pub fn open_vault_file(path: &Path, vault_root: &Path) -> Result<(), String> {
    launch(check_vault_file(path, vault_root)?.as_ref())
}

/// Open the PDF that was just exported (it may be outside the vault, where the user chose).
pub fn open_exported_pdf(path: &Path) -> Result<(), String> {
    launch(check_exported_pdf(path)?.as_ref())
}

/// Open a folder of rescued notes (see `recovery`) in the file manager. Only folders inside the
/// recovery folder are opened.
pub fn open_recovery_folder(path: &Path, recovery_root: &Path) -> Result<(), String> {
    if !crate::recovery::is_inside(recovery_root, path) {
        return Err("Not opened: not a folder of rescued notes".into());
    }
    launch(path.as_os_str())
}

/// Save `data` (a mail attachment, say) as `name` in Shale's private temp folder and open that
/// copy. Each call uses a fresh folder, so same-named attachments never clash.
pub fn open_temp_copy(name: &str, data: &[u8]) -> Result<(), String> {
    let path = save_temp_copy(name, data)?;
    launch(check_temp_file(&path)?.as_ref())
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
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "txt", "md", "csv", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "mp3", "mp4", "m4a", "wav", "ogg", "flac", "mkv", "webm", "mov", "avi", "eml",
    "ics", "vcf", "zip", "epub",
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

/// The exported PDF to open: a PDF file that is not executable.
pub fn check_exported_pdf(path: &Path) -> Result<PathBuf, String> {
    let real = real_file(path)?;
    if extension(&real) != "pdf" {
        return Err("Not opened: not a PDF".into());
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

/// Windows: hand the address or file to the shell's "open" verb. No command line is built, so
/// nothing in `target` can be taken for a switch or a second command (SEC-005).
#[cfg(windows)]
fn launch_windows(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let raw: Vec<u16> = target.encode_wide().collect();
    let file: Vec<u16> = strip_verbatim(&raw).into_iter().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
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
    // tests: the pretend program gets what would be opened (nothing starts)
    #[cfg(test)]
    if let Some(b) = test_browser().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        b(&target.to_string_lossy());
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
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(target).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(target).spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let r: std::io::Result<std::process::Child> = Err(std::io::Error::other("unsupported"));
    match r {
        Ok(mut child) => {
            // reap it in the background so it never stays a zombie
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(e) => Err(format!("Could not open it: {e}")),
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

    // spec: SEC-006
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
                    if !l.starts_with("//") && (l.contains("\"xdg-open\"") || l.contains("Command::new(\"open\")") || l.contains("\"/C\"") || l.contains("ShellExecute")) {
                        bad.push(format!("{name}:{}: {l}", i + 1));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "open things through opener instead:\n{}", bad.join("\n"));
    }
}
