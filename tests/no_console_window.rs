//! No code starts an external program without `program::command` (BUILD-043): on Windows that
//! helper sets `CREATE_NO_WINDOW`, so no console window flashes up for a `git`, `ffmpeg`, `gpg` or
//! `curl` run of the program with a window (BUILD-042).

use std::fs;
use std::path::{Path, PathBuf};

/// The file with the helper `command` and the exception `relaunch_command`: the only production
/// code that may call `Command::new`, and exactly twice (one call each).
const HELPER: &str = "src/program.rs";

/// Whether a file under `src/` is test code as a whole (`x_tests.rs`, `tests.rs`, `tests/…`, `ui_test_support.rs`).
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.ends_with("tests.rs") || name.starts_with("ui_test") || rel.contains("/tests/")
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// How much more `(`, `[` and `{` than closers a line has.
fn open_count(line: &str) -> i32 {
    line.chars()
        .map(|c| {
            if "([{".contains(c) {
                1
            } else if ")]}".contains(c) {
                -1
            } else {
                0
            }
        })
        .sum()
}

/// The lines of production code with their numbers: those that are not in an item behind
/// `#[cfg(test)]` (or `#[cfg(all(test, …))]`: a `mod tests`, a test helper, a field or a
/// statement) and are not comments. An item that fits one line (balanced brackets) ends there; a
/// longer one ends at the first line that closes it at the indent of its first line (rustfmt,
/// DEV-015, writes it so).
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("#[cfg(test)]") || t.starts_with("#[cfg(all(test") {
            let mut first = i + 1;
            while first < lines.len() && lines[first].trim_start().starts_with("#[") {
                first += 1;
            }
            let Some(head) = lines.get(first) else { break };
            i = if open_count(head) == 0 {
                first + 1
            } else {
                let n = indent(head);
                (first + 1..lines.len()).find(|&k| indent(lines[k]) == n && lines[k].trim_start().starts_with(['}', ')', ']'])).map_or(lines.len(), |k| k + 1)
            };
            continue;
        }
        if !t.starts_with("//") {
            out.push((i + 1, lines[i]));
        }
        i += 1;
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `file:line: text` for every production `Command::new(` outside the helper's file, and the
/// number of calls inside it.
fn starts(files: &[(String, String)]) -> (Vec<String>, usize) {
    let (mut bad, mut in_helper) = (Vec::new(), 0);
    for (rel, text) in files {
        if is_test_file(rel) {
            continue;
        }
        for (n, line) in production_lines(text) {
            if line.contains("Command::new(") {
                if rel == HELPER {
                    in_helper += 1;
                } else {
                    bad.push(format!("{rel}:{n}: {}", line.trim()));
                }
            }
        }
    }
    (bad, in_helper)
}

// spec: BUILD-043
#[test]
fn build_043_no_code_starts_a_program_without_the_helper() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    sources(&root.join("src"), &mut paths);
    paths.sort();
    let files: Vec<(String, String)> = paths.iter().map(|p| (p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), fs::read_to_string(p).unwrap())).collect();
    let users = files.iter().filter(|(rel, t)| !is_test_file(rel) && rel != HELPER && production_lines(t).iter().any(|(_, l)| l.contains("program::command("))).count();
    assert!(users >= 8, "the code that starts programs is found ({users} files)");
    let (bad, in_helper) = starts(&files);
    assert!(bad.is_empty(), "start programs with `crate::program::command(…)`, so no console window flashes up on Windows (BUILD-043):\n{}", bad.join("\n"));
    assert_eq!(in_helper, 2, "{HELPER} has `command` and the one exception `relaunch_command`, nothing else");
}

// spec: BUILD-043
#[test]
fn build_043_the_scan_leaves_test_code_alone_and_finds_the_rest() {
    let src = "\
fn run() {
    let c = Command::new(\"git\");
    // Command::new(\"in a comment\");
}
#[cfg(test)]
static FAKE: Mutex<Option<u8>> = Mutex::new(None);
fn more() {
    #[cfg(test)]
    if let Some(f) = fake() {
        c = Command::new(\"sh\");
    }
    #[cfg(test)]
    let t = Command::new(\"sh\");
    let d = Command::new(\"curl\");
}
struct S {
    #[cfg(test)]
    pub fake: Option<u8>,
    x: u8,
}
#[cfg(test)]
mod tests {
    fn t() {
        Command::new(\"sh\");
    }
}
#[cfg(all(test, unix))]
thread_local! {
    static A: u8 = Command::new(\"sh\");
}
fn last() {
    Command::new(\"ffmpeg\");
}
";
    let found: Vec<usize> = production_lines(src).into_iter().filter(|(_, l)| l.contains("Command::new(")).map(|(n, _)| n).collect();
    assert_eq!(found, [2, 14, 32], "the lines of production code that start a program");
    let files = [
        ("src/a.rs".to_string(), "fn f() { Command::new(\"git\"); }\n#[cfg(test)]\nmod tests {\n    fn t() { Command::new(\"sh\"); }\n}\n".to_string()),
        ("src/a_tests.rs".to_string(), "fn t() { Command::new(\"sh\"); }\n".to_string()),
        ("src/b/tests/c.rs".to_string(), "fn t() { Command::new(\"sh\"); }\n".to_string()),
        (HELPER.to_string(), "pub fn command() { Command::new(x); }\n".to_string()),
    ];
    let (bad, in_helper) = starts(&files);
    assert_eq!(bad.len(), 1, "only the production code of a.rs: {bad:?}");
    assert!(bad[0].starts_with("src/a.rs:1:"));
    assert_eq!(in_helper, 1);
}
