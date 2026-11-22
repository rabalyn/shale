//! The limits of `scripts/docx-corpus.sh` (DEV-037, #528). The script's real work needs the network and builds the
//! test binary; here a stand-in for that binary (what `SHALE_ROUNDTRIP_BIN` is for) ends the way the real one does
//! when pandoc was stopped, and the script must report the file and go on. Linux only: the scripts are bash.
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new("sh").arg("-c").arg(format!("command -v {tool}")).output().is_ok_and(|o| o.status.success())
}

fn sha256(bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("sha256sum").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().split_whitespace().next().unwrap().to_string()
}

// spec: DEV-037
#[test]
fn dev_037_a_file_for_which_pandoc_gave_up_is_reported_and_the_run_goes_on() {
    if let Some(missing) = ["pandoc", "curl", "sha256sum", "python3", "xmllint", "unzip"].into_iter().find(|t| !have(t)) {
        eprintln!("skipping: {missing} is not installed here");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir: PathBuf = std::env::temp_dir().join(format!("shale-corpus-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // the stand-in: by the file it is given, it ends like the real test binary when pandoc was stopped by its memory limit
    // (Shale's message), by its time limit (the script's `timeout`), or works, or fails for another reason
    let bin = dir.join("fake-bin");
    std::fs::write(
        &bin,
        "#!/bin/sh\n\
         case \"$(basename \"$SHALE_ROUNDTRIP_DOCX\")\" in\n\
           heap.docx) echo 'import error: pandoc needed more than 2 GB of memory for “heap.docx” and was stopped'; exit 101;;\n\
           slow.docx) sleep 8; exit 0;;\n\
           bad.docx) echo 'import error: pandoc could not read “bad.docx”: damaged'; exit 101;;\n\
           *) echo 'words that came back: 3 of 3 (100.0 %); the export has 3'; exit 0;;\n\
         esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let names = ["heap", "slow", "bad", "fine"];
    let mut list = String::new();
    for name in names {
        let file = dir.join(format!("{name}.docx.src"));
        let bytes = format!("not a real docx: {name}").into_bytes();
        std::fs::write(&file, &bytes).unwrap();
        list.push_str(&format!("{name}\tfile://{}\t{}\ttest\n", file.display(), sha256(&bytes)));
    }
    let list_file = dir.join("list");
    std::fs::write(&list_file, list).unwrap();
    let keep = dir.join("keep");
    let out = Command::new("bash")
        .arg(root.join("scripts/docx-corpus.sh"))
        .args(["--no-pages", "--list"])
        .arg(&list_file)
        .arg("--keep")
        .arg(&keep)
        .current_dir(root)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("SHALE_ROUNDTRIP_BIN", &bin)
        .env("SHALE_CORPUS_NO_FETCH", "1")
        .env("SHALE_CORPUS_TIMEOUT", "2")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("bash runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = |name: &str| text.lines().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("no line for {name}: {text}")).to_string();
    assert!(line("heap").ends_with("pandoc gave up (memory or time)"), "{text}");
    assert!(line("slow").ends_with("pandoc gave up (memory or time)"), "the round trip is cut off by the timeout: {text}");
    assert!(line("bad").ends_with("import error: pandoc could not read “bad.docx”: damaged"), "{text}");
    assert!(line("fine").contains("100.0%") && !line("fine").contains("gave up"), "{text}");
    assert_eq!(out.status.code(), Some(3), "the three with an error count, the run went through all four: {text}");
    assert!(text.contains("4 files, 3 with a problem"), "{text}");
    let summary = std::fs::read_to_string(keep.join("summary.tsv")).unwrap();
    assert_eq!(summary.matches("pandoc gave up (memory or time)").count(), 2, "{summary}");
    let _ = std::fs::remove_dir_all(&dir);
}
