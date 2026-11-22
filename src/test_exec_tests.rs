//! `test_exec::write_script`: a script written by a test can be started at once, also while other
//! threads fork (DEV-040).

use crate::test_exec::write_script;
use std::path::PathBuf;
use std::process::Command;

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-test-exec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// spec: DEV-040
#[test]
fn dev_040_the_script_is_started_once_without_its_lines_and_then_runs_them() {
    let d = temp("lines");
    let mark = d.join("ran");
    let script = d.join("bin/tool");
    write_script(&script, &format!("#!/bin/sh\nset -eu\necho \"$1\" >> '{}'\n", mark.display()));
    assert!(!mark.exists(), "the first start only makes sure that the file can be started");
    let out = Command::new(&script).arg("one").output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&mark).unwrap(), "one\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DEV-040
#[test]
#[should_panic(expected = "a test script starts with #!/bin/sh")]
fn dev_040_a_script_for_another_interpreter_is_refused() {
    write_script(&temp("refused").join("tool"), "#!/usr/bin/env python3\nprint(1)\n");
}

// spec: DEV-040
#[test]
fn dev_040_a_script_is_never_busy_while_other_threads_fork() {
    let d = temp("forks");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let forkers: Vec<_> = (0..4)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = Command::new("true").status();
                }
            })
        })
        .collect();
    let writers: Vec<_> = (0..4)
        .map(|n| {
            let d = d.clone();
            std::thread::spawn(move || {
                for i in 0..40 {
                    let script = d.join(format!("tool-{n}-{i}"));
                    write_script(&script, "#!/bin/sh\necho started\n");
                    let out = Command::new(&script).output().unwrap_or_else(|e| panic!("{}: {e}", script.display()));
                    assert_eq!(String::from_utf8_lossy(&out.stdout), "started\n");
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for f in forkers {
        f.join().unwrap();
    }
    let _ = std::fs::remove_dir_all(&d);
}
