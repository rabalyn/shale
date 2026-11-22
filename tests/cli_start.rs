//! The command line of the real program is checked before a window opens (BUILD-019, DEV-013):
//! a wrong one ends the start with the message and the usage on stderr and exit code 2, and no
//! vault folder is made from a word that was meant as the value of an option.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A fresh working folder, so that a folder the program wrongly made shows up in it.
fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-cli-start-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn shale(dir: &PathBuf, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_shale"));
    // no display at all: a run that tried to open a window would fail with another message
    c.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").current_dir(dir).args(args);
    c.output().unwrap()
}

fn entries(dir: &PathBuf) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

// spec: BUILD-019, DEV-013
#[test]
fn build_019_a_wrong_command_line_ends_the_start_with_the_message_the_usage_and_exit_code_2() {
    let dir = workdir("bad");
    for (args, message) in [
        (&["--nonsense"][..], "shale: unknown option --nonsense\n"),
        (&["--sise", "1360x840"], "shale: unknown option --sise\n"),
        (&["--vault"], "shale: --vault needs a value\n"),
        (&["--screenshot", "out.png", "--view"], "shale: --view needs a value\n"),
        (&["--size", "1360"], "shale: --size needs WIDTHxHEIGHT (at least 1280x720), e.g. 1360x840, not 1360\n"),
        (&["--size", "100x100"], "shale: --size needs WIDTHxHEIGHT (at least 1280x720), e.g. 1360x840, not 100x100\n"),
        (&["notes", "more"], "shale: one vault folder only, got notes and more\n"),
    ] {
        let run = shale(&dir, args);
        let (out, err) = (String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        assert_eq!(run.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.starts_with(message), "{args:?}: {err}");
        assert!(err.contains("\nusage: shale "), "{args:?}: the usage follows the message: {err}");
        assert_eq!(out, "", "{args:?}: nothing on stdout");
    }
    assert_eq!(entries(&dir), 0, "no word of a wrong command line became a vault folder");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: DEV-013
#[test]
fn dev_013_help_prints_the_usage_and_exits_with_0() {
    let dir = workdir("help");
    for args in [&["-h"][..], &["--help"], &["--vault", "x", "--help"]] {
        let run = shale(&dir, args);
        assert!(run.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&run.stdout).starts_with("usage: shale "), "{args:?}");
        assert!(run.stderr.is_empty(), "{args:?}");
    }
    assert_eq!(entries(&dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
