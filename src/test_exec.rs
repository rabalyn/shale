//! A shell script a test writes to stand in for an external program (gpg, pandoc, ffprobe, git's
//! hooks, a keyring tool), and that something starts a moment later (DEV-040).
//!
//! Starting a file that was written just now can fail with "Text file busy" (`ETXTBSY`): another
//! test thread that forks while the file is open for writing keeps a copy of the open file in its
//! child until that child runs its program, and the system refuses to run a file that is open for
//! writing anywhere. It passes within milliseconds, but whatever starts the script later (git for
//! a hook, a program under test, the test itself) does not try again. So `write_script` runs the
//! new script once itself, before it returns, and tries again while it is busy: the script ends
//! at once on that first run (before its own lines, when `SHALE_TEST_WARM_UP` is set), and after
//! it nothing holds the file open for writing any more.
//!
//! This file is a module of the program's tests and, with `#[path]`, of the integration tests in
//! `tests/`, so it names nothing of the program.
#![cfg(test)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The variable that makes a script written by `write_script` end before its own lines.
const WARM_UP: &str = "SHALE_TEST_WARM_UP";

/// Write `text`, a script whose first line is `#!/bin/sh` or `#!/bin/bash`, to `path` as an
/// executable (its folder is made when missing) and return when it can be started.
pub fn write_script(path: &Path, text: &str) {
    let (shebang, rest) = text.split_once('\n').unwrap_or((text, ""));
    assert!(shebang == "#!/bin/sh" || shebang == "#!/bin/bash", "a test script starts with #!/bin/sh or #!/bin/bash, not {shebang:?}");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, format!("{shebang}\n[ -z \"${{{WARM_UP}-}}\" ] || exit 0\n{rest}")).unwrap();
    make_executable(path);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match Command::new(path).env(WARM_UP, "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Err(e) if is_busy(&e) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("{} cannot be started: {e}", path.display()),
            Ok(status) => {
                assert!(status.success(), "the first start of {} ended with {status}", path.display());
                return;
            }
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_: &Path) {}

#[cfg(unix)]
fn is_busy(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn is_busy(_: &std::io::Error) -> bool {
    false
}
