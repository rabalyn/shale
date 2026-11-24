//! `shale export-site <vault> <out>` (SITE-011, SITE-012, SITE-015): the real program, with no window.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn shale(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_shale"));
    // no display at all: a run that tried to open a window would fail; and the settings of this computer are not read
    c.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").env("XDG_CONFIG_HOME", dir.join("config")).env("XDG_DATA_HOME", dir.join("data")).env("HOME", dir.join("home"));
    c
}

fn vault(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-site-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("vault/.shale")).unwrap();
    std::fs::write(dir.join("vault/.shale/settings.json"), r#"{"version": 1, "publish": {"enabled": true, "include_tags": ["pub"]}}"#).unwrap();
    std::fs::write(dir.join("vault/Open.md"), "#pub\nSee [[Closed]] and [[Other]].\n").unwrap();
    std::fs::write(dir.join("vault/Other.md"), "#pub\nOther.\n").unwrap();
    std::fs::write(dir.join("vault/Closed.md"), "Closed.\n").unwrap();
    dir
}

fn stdout(run: &Output) -> String {
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn stderr(run: &Output) -> String {
    String::from_utf8_lossy(&run.stderr).into_owned()
}

// spec: SITE-011, SITE-015
#[test]
fn export_site_writes_the_tree_says_what_it_wrote_and_exits_with_0() {
    let dir = vault("ok");
    let out = dir.join("site");
    let run = shale(&dir).arg("export-site").arg(dir.join("vault")).arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run).lines().last(), Some("Wrote 2 notes, 0 media files and 1 link; 1 link became plain text."));
    assert!(out.join("hugo.toml").is_file() && out.join("content/open.md").is_file() && out.join("themes/shale/theme.toml").is_file() && out.join(".shale-export.json").is_file());
    assert!(!out.join("content/closed.md").exists());
    assert!(!dir.join("vault/.shale/state.json").exists(), "the vault is read, not changed");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: SITE-009, SITE-011
#[test]
fn export_site_dry_run_lists_what_it_would_write_and_writes_nothing() {
    let dir = vault("dry");
    let out = dir.join("site");
    let run = shale(&dir).arg("export-site").arg("--dry-run").arg(dir.join("vault")).arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    let text = stdout(&run);
    assert!(text.contains("note Open.md -> content/open.md\nnote Other.md -> content/other.md\n"), "{text}");
    assert!(text.lines().last().unwrap().starts_with("Would write 2 notes"));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: SITE-012
#[test]
fn export_site_says_why_it_failed_and_exits_with_1_and_writes_nothing() {
    let dir = vault("bad");
    let out = dir.join("site");
    let run = shale(&dir).arg("export-site").arg(dir.join("nothing")).arg(&out).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("is not a folder"), "{}", stderr(&run));
    for args in [vec!["export-site"], vec!["export-site", "only-one"], vec!["export-site", "a", "b", "c"]] {
        let run = shale(&dir).args(args).output().unwrap();
        assert_eq!(run.status.code(), Some(1));
        assert!(stderr(&run).contains("usage: shale export-site"), "{}", stderr(&run));
    }
    // --take-over belongs to --publish
    let run = shale(&dir).args(["export-site", "--take-over"]).arg(dir.join("vault")).arg(&out).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("unknown option --take-over"), "{}", stderr(&run));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: SITE-066, SITE-076
#[test]
fn export_site_publish_pushes_the_site_to_the_target_of_the_settings_and_says_why_it_cannot() {
    let dir = vault("publish");
    let remote = dir.join("remote.git");
    let init = Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).output().unwrap();
    assert!(init.status.success());
    // without an address the command says so and pushes nothing
    let run = shale(&dir).args(["export-site", "--publish"]).arg(dir.join("vault")).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("No address is set to publish to"), "{}", stderr(&run));
    // with one it pushes: the summary on stdout, one commit on the branch of the settings
    let settings = serde_json::json!({"version": 1, "publish": {"enabled": true, "include_tags": ["pub"], "url": remote.to_string_lossy(), "branch": "site"}});
    std::fs::write(dir.join("vault/.shale/settings.json"), settings.to_string()).unwrap();
    let run = shale(&dir).args(["export-site", "--publish"]).arg(dir.join("vault")).output().unwrap();
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run).lines().last(), Some("Published 2 notes, 0 media files and 1 link; 1 link became plain text."));
    assert!(stderr(&run).contains("shale: Publishing…"), "{}", stderr(&run));
    let log = Command::new("git").arg("-C").arg(&remote).args(["log", "--format=%s", "site"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&log.stdout).lines().collect::<Vec<_>>(), ["Publish vault: 2 notes"]);
    let files = Command::new("git").arg("-C").arg(&remote).args(["ls-tree", "-r", "--name-only", "site"]).output().unwrap();
    let files = String::from_utf8_lossy(&files.stdout).into_owned();
    assert!(files.contains("content/open.md") && !files.contains("closed"), "{files}");
    assert!(!dir.join("vault/.git").exists(), "the vault is no repository");
    // a folder that is not there: the reason and the exit code
    let run = shale(&dir).args(["export-site", "--publish"]).arg(dir.join("nothing")).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("is not a folder") && stderr(&run).contains("usage: shale export-site"), "{}", stderr(&run));
    let _ = std::fs::remove_dir_all(&dir);
}
