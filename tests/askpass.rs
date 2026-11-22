//! Git's askpass helper, the real Shale executable against the real git (SYNC-054, ADR-0029).
//! A pretend `secret-tool` (tests/askpass/secret-tool) is the keyring, so the token is read
//! the way a Linux desktop reads it, from a process that is not the test.
//!
//! Linux only: `secret-tool` is the one keyring backend a separate process can fake. macOS uses
//! the Keychain API and Windows the Credential Manager; there the helper's prompt parsing and
//! lookup are covered by the unit tests in `src/gitsync/credentials_tests.rs` with an injected
//! keyring.

#![cfg(target_os = "linux")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "../src/test_exec.rs"]
mod test_exec;

struct Setup {
    root: PathBuf,
    path: std::ffi::OsString,
}

impl Setup {
    fn new(tag: &str) -> Setup {
        let root = std::env::temp_dir().join(format!("shale-askpass-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("secrets")).unwrap();
        let fake = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/askpass/secret-tool")).unwrap();
        test_exec::write_script(&root.join("bin/secret-tool"), &fake);
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(std::iter::once(root.join("bin")).chain(std::env::split_paths(&rest))).unwrap();
        Setup { root, path }
    }

    fn secret_tool(&self, args: &[&str], stdin: &str) {
        let mut c = Command::new(self.root.join("bin/secret-tool"));
        c.args(args).env("FAKE_SECRET_DIR", self.root.join("secrets")).stdin(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
    }

    fn store(&self, server: &str, user: &str, token: &str) {
        self.secret_tool(&["store", "--label", "test", "application", "shale-git", "server", server, "user", user], token);
    }

    /// Shale started the way git starts its askpass program.
    fn askpass(&self, prompt: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_shale")).arg(prompt).env("SHALE_GIT_ASKPASS", "1").env("PATH", &self.path).env("FAKE_SECRET_DIR", self.root.join("secrets")).stdin(Stdio::null()).output().unwrap()
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// spec: SYNC-054
#[test]
fn sync_054_the_executable_answers_a_password_prompt_from_the_keyring_and_nothing_else() {
    let s = Setup::new("direct");
    s.store("https://git.test", "ada", "tok-ada");
    s.store("https://git.test:8443", "ada", "tok-port");
    let ok = s.askpass("Password for 'https://ada@git.test': ");
    assert!(ok.status.success());
    assert_eq!(String::from_utf8_lossy(&ok.stdout), "tok-ada\n");
    assert_eq!(String::from_utf8_lossy(&s.askpass("Password for 'https://ada@git.test:8443': ").stdout), "tok-port\n");
    assert_eq!(String::from_utf8_lossy(&s.askpass("Password for 'https://ada@GIT.TEST:443': ").stdout), "tok-ada\n", "host case and the default port do not matter");
    // another user, another host, a user name prompt, a prompt that is not git's: no answer, exit 1
    for prompt in ["Password for 'https://bob@git.test': ", "Password for 'https://ada@other.test': ", "Username for 'https://git.test': ", "Password for 'https://git.test': "] {
        let o = s.askpass(prompt);
        assert_eq!(o.status.code(), Some(1), "{prompt}");
        assert!(o.stdout.is_empty(), "{prompt}");
    }
}

// spec: SYNC-054
#[test]
fn sync_054_git_gets_the_token_from_the_helper_and_without_one_it_fails_at_once() {
    let s = Setup::new("git");
    s.store("https://git.test", "ada", "tok-ada");
    let credential_fill = |input: &str| {
        let mut c = Command::new("git");
        c.args(["credential", "fill"])
            .env("GIT_ASKPASS", env!("CARGO_BIN_EXE_shale"))
            .env("SHALE_GIT_ASKPASS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PATH", &s.path)
            .env("FAKE_SECRET_DIR", s.root.join("secrets"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    };
    let o = credential_fill("protocol=https\nhost=git.test\nusername=ada\n\n");
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && text.contains("password=tok-ada"), "{text} {}", String::from_utf8_lossy(&o.stderr));
    let o = credential_fill("protocol=https\nhost=git.test\nusername=bob\n\n");
    assert!(!o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("password="), "{}", String::from_utf8_lossy(&o.stderr));
}
