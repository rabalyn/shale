//! Tests of the memory limit of pandoc (IMP-062, DOCX-012).

use crate::pandoc_limit::{overflow_message, overflowed, run_with, MAX_HEAP};
use crate::pdf::tex::proc;
use crate::pdf::tex::proc::{Ending, Run};
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn run_of(ending: Ending, tail: &[&str]) -> Run {
    Run { ending, tail: tail.iter().map(|l| l.to_string()).collect() }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-pandoclimit-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn go(cmd: Command) -> io::Result<Run> {
    proc::run(cmd, Duration::from_secs(20), None, |_| {})
}

// spec: IMP-062
#[cfg(unix)]
#[test]
fn the_limit_is_two_gigabytes_set_in_the_environment_of_the_command() {
    assert_eq!((MAX_HEAP, crate::pandoc_limit::MAX_HEAP_TEXT), ("2g", "2 GB"));
    // a stand-in that prints its environment: the limit is there, last, so it wins over one of the user's
    let dir = temp_dir("env");
    let printer = dir.join("pandoc");
    crate::test_exec::write_script(&printer, "#!/bin/sh\necho \"$GHCRTS\"");
    let run = crate::pandoc_limit::run(&printer, |_| {}, go).unwrap();
    assert!(run.tail[0].ends_with("-M2g"), "{run:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: IMP-062
#[cfg(unix)]
#[test]
fn run_starts_pandoc_with_the_limit_and_a_pandoc_that_refuses_runtime_options_is_run_again_without_it_once() {
    let dir = temp_dir("refused");
    let seen = dir.join("seen");
    // records GHCRTS of each start; refuses it like a pandoc built without -rtsopts
    let refusing = dir.join("pandoc-refusing");
    crate::test_exec::write_script(
        &refusing,
        &format!("#!/bin/sh\necho \"[$GHCRTS]\" >> '{}'\nif [ -n \"$GHCRTS\" ]; then echo 'pandoc: Most RTS options are disabled. Link with -rtsopts to enable them.' >&2; exit 1; fi\necho fine", seen.display()),
    );
    let first_run = crate::pandoc_limit::run(
        &refusing,
        |cmd| {
            cmd.arg("x");
        },
        go,
    )
    .unwrap();
    assert!(first_run.success() && first_run.tail == ["fine"], "{first_run:?}");
    let first: Vec<String> = std::fs::read_to_string(&seen).unwrap().lines().map(String::from).collect();
    assert!(first.len() == 2 && first[0].ends_with("-M2g]") && first[1] == "[]", "tried with the limit, then without: {first:?}");
    // remembered: the next run is without the limit at the first start
    assert!(run_of_again(&refusing).success());
    let all = std::fs::read_to_string(&seen).unwrap();
    assert_eq!(all.lines().count(), 3, "{all}");
    // a pandoc that takes it is run once, with the limit
    let seen2 = dir.join("seen2");
    let good = dir.join("pandoc-good");
    crate::test_exec::write_script(&good, &format!("#!/bin/sh\necho \"[$GHCRTS]\" >> '{}'", seen2.display()));
    assert!(crate::pandoc_limit::run(&good, |_| {}, go).unwrap().success());
    assert_eq!(std::fs::read_to_string(&seen2).unwrap().lines().count(), 1);
    // a failure that is not the refusal is no reason to run again
    let failing = dir.join("pandoc-failing");
    crate::test_exec::write_script(&failing, &format!("#!/bin/sh\necho x >> '{}'; echo 'Error: damaged' >&2; exit 1", seen2.display()));
    assert!(!crate::pandoc_limit::run(&failing, |_| {}, go).unwrap().success());
    assert_eq!(std::fs::read_to_string(&seen2).unwrap().lines().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
fn run_of_again(pandoc: &std::path::Path) -> Run {
    crate::pandoc_limit::run(pandoc, |_| {}, go).unwrap()
}

// spec: IMP-062
#[test]
fn a_heap_overflow_is_known_by_its_exit_code_or_its_message_and_nothing_else_is() {
    let message = ["pandoc: Heap exhausted;", "pandoc: Current maximum heap size is 2147483648 bytes (2048 MB).", "pandoc: Use `+RTS -M<size>' to increase it."];
    assert!(overflowed(&run_of(Ending::Exited(Some(251)), &message)));
    assert!(overflowed(&run_of(Ending::Exited(Some(251)), &[])));
    assert!(overflowed(&run_of(Ending::Exited(Some(1)), &["pandoc: heap overflow"])));
    assert!(!overflowed(&run_of(Ending::Exited(Some(0)), &message)), "a run that succeeded did not overflow");
    assert!(!overflowed(&run_of(Ending::Exited(Some(64)), &["Error: the file is damaged"])));
    assert!(!overflowed(&run_of(Ending::TimedOut, &message)) && !overflowed(&run_of(Ending::Cancelled, &message)));
    assert_eq!(overflow_message(Some("a.docx")), "pandoc needed more than 2 GB of memory for “a.docx” and was stopped");
    assert_eq!(overflow_message(None), "pandoc needed more than 2 GB of memory and was stopped");
}

// spec: IMP-062, DOCX-012, BUILD-018
#[test]
fn the_real_pandoc_stops_at_a_tiny_limit_with_the_message_that_is_recognized() {
    let Some(pandoc) = crate::import::docx::locate("") else {
        eprintln!("skipping: pandoc is not installed here");
        return;
    };
    let dir = temp_dir("real");
    let md = dir.join("small.md");
    std::fs::write(&md, "# Title\n\nSome *text* in a small file.\n").unwrap();
    let docx = dir.join("small.docx");
    let made = run_with(
        &pandoc,
        "64m",
        |cmd| {
            cmd.arg(&md).args(["-t", "docx", "-o"]).arg(&docx);
        },
        go,
    )
    .unwrap();
    assert!(made.success(), "a normal limit lets it through: {made:?}");
    // 4 MB is less than pandoc needs to read a Word file
    let run = run_with(
        &pandoc,
        "4m",
        |cmd| {
            cmd.arg(&docx).args(["-f", "docx", "-t", "gfm"]);
        },
        go,
    )
    .unwrap();
    assert!(overflowed(&run), "{run:?}");
    assert!(run.tail.iter().any(|l| l.contains("Heap exhausted")), "{run:?}");
    // and the limit the program uses does not get in the way
    assert!(run_with(
        &pandoc,
        MAX_HEAP,
        |cmd| {
            cmd.arg(&docx).args(["-f", "docx", "-t", "gfm"]);
        },
        go
    )
    .unwrap()
    .success());
    let _ = std::fs::remove_dir_all(&dir);
}
