//! No code starts a shell (SEC-005): external programs get an argument list, so nothing a note,
//! a mail or a server says can become a command.

use std::fs;
use std::path::{Path, PathBuf};

/// The shells and command interpreters; as the program of a `Command::new(…)`.
const SHELLS: [&str; 10] = ["sh", "bash", "zsh", "dash", "fish", "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh"];

/// Quoted words that only a shell command line needs (`cmd /C start`, `powershell -Command`).
const SHELL_FLAGS: [&str; 5] = ["\"/C\"", "\"/c\"", "\"/K\"", "\"-Command\"", "\"-EncodedCommand\""];

/// Text that is a command line for a shell, inside any string.
const SHELL_LINES: [&str; 8] = ["sh -c", "bash -c", "zsh -c", "cmd /c", "cmd /C", "cmd.exe /", "powershell -", "pwsh -"];

/// The files that may start a shell; none today. A file here needs its reason.
const ALLOWED: [&str; 0] = [];

/// What is wrong with a line of production code, if anything.
fn shell_start(line: &str) -> Option<String> {
    let l = line.trim_start();
    if l.starts_with("//") {
        return None;
    }
    for at in l.match_indices("Command::new(").map(|(i, _)| i + "Command::new(".len()) {
        let arg = l[at..].trim_start_matches('&');
        if arg.starts_with("format!") {
            return Some("a program name built with format!".into());
        }
        if let Some(name) = arg.strip_prefix('"').and_then(|s| s.split('"').next()) {
            let program = name.rsplit(['/', '\\']).next().unwrap_or(name);
            if SHELLS.contains(&program.to_ascii_lowercase().as_str()) {
                return Some(format!("the shell {name:?} is started"));
            }
        }
    }
    SHELL_FLAGS.iter().chain(SHELL_LINES.iter()).find(|f| l.contains(*f)).map(|f| format!("{f} is a shell command line"))
}

/// The production code of a source file: what comes before its `#[cfg(test)]` module.
fn production(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or("")
}

/// Whether a file under `src/` is test code as a whole (`x_tests.rs`, `tests.rs`, `tests/…`, `ui_test_support.rs`).
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.ends_with("tests.rs") || name.starts_with("ui_test") || rel.contains("/tests/")
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

/// `file:line: problem` for every shell start in `(path from the repo, text)` files.
fn shell_starts(files: &[(String, String)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, text) in files {
        if is_test_file(rel) || ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        for (n, line) in production(text).lines().enumerate() {
            if let Some(why) = shell_start(line) {
                bad.push(format!("{rel}:{}: {why}: {}", n + 1, line.trim()));
            }
        }
    }
    bad
}

// spec: SEC-005
#[test]
fn sec_005_no_code_starts_a_shell() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    sources(&root.join("src"), &mut paths);
    paths.sort();
    let files: Vec<(String, String)> = paths.iter().map(|p| (p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), fs::read_to_string(p).unwrap())).collect();
    let programs = files.iter().filter(|(rel, t)| !is_test_file(rel) && production(t).contains("Command::new(")).count();
    assert!(programs >= 8, "the code that starts programs is found ({programs} files)");
    let bad = shell_starts(&files);
    assert!(bad.is_empty(), "programs are started with an argument list, never through a shell (SEC-005):\n{}", bad.join("\n"));
    assert!(ALLOWED.is_empty(), "no file is allowed to start a shell");
}

// spec: SEC-005
#[test]
fn sec_005_the_scan_finds_a_shell_and_leaves_argument_lists_alone() {
    for bad in [
        r#"let mut c = Command::new("sh");"#,
        r#"Command::new("/bin/bash").arg("-c").arg(cmdline)"#,
        r#"std::process::Command::new("cmd").args(["/C", "start", url])"#,
        r#"Command::new("cmd.exe")"#,
        r#"let c = Command::new(&format!("{dir}/tool"));"#,
        r#"Command::new("powershell").arg("-Command")"#,
        r#"x.args(["/C", "start"])"#,
        r#"let line = format!("sh -c 'open {path}'");"#,
    ] {
        assert!(shell_start(bad).is_some(), "{bad}");
    }
    for ok in [
        r#"let mut c = Command::new("curl");"#,
        r#"Command::new(&self.program).args(["-c", "user.name=Shale"])"#,
        r#"Command::new(program).arg("sh")"#,
        r#"Command::new("shasum").arg("-a")"#,
        r#"// Command::new("sh") would be wrong"#,
        r#"const PROGRAM_EXTENSIONS: &[&str] = &["sh", "bash", "cmd"];"#,
        r#"let git = Command::new("cmd/git.exe");"#,
    ] {
        assert_eq!(shell_start(ok), None, "{ok}");
    }
    let files = [
        ("src/a.rs".to_string(), "fn run() { Command::new(\"sh\"); }\n#[cfg(test)]\nmod t { fn x() { Command::new(\"sh\"); } }\n".to_string()),
        ("src/a_tests.rs".to_string(), "fn x() { Command::new(\"sh\"); }\n".to_string()),
        ("src/b/tests/c.rs".to_string(), "fn x() { Command::new(\"bash\"); }\n".to_string()),
    ];
    let bad = shell_starts(&files);
    assert_eq!(bad.len(), 1, "only the production code of a.rs: {bad:?}");
    assert!(bad[0].starts_with("src/a.rs:1:"));
}
