//! `shale --export-pdf <folder> <file.pdf>` (BOOK-011): the real program, with no window.

use std::process::Command;

fn shale() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_shale"));
    // no display at all: a run that tried to open a window would fail
    c.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    c
}

// spec: BOOK-011
#[test]
fn export_pdf_writes_the_book_and_exits_with_0() {
    let dir = std::env::temp_dir().join(format!("shale-export-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Book")).unwrap();
    std::fs::write(dir.join("Book/index.md"), "---\ntitle: A book\n---\nHello. See [[One]].\n").unwrap();
    std::fs::write(dir.join("Book/One.md"), "# One\n\nFirst chapter.\n").unwrap();
    let out = dir.join("out/book.pdf");
    let run = shale().arg("--export-pdf").arg(dir.join("Book")).arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    assert!(!dir.join("Book/.shale").exists(), "the folder is read, not changed");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: BOOK-011
#[test]
fn export_pdf_says_why_it_failed_and_exits_with_1() {
    let dir = std::env::temp_dir().join(format!("shale-export-cli-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Empty")).unwrap();
    let run = shale().arg("--export-pdf").arg(dir.join("Nothing")).arg(dir.join("a.pdf")).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("is not a folder"), "{}", String::from_utf8_lossy(&run.stderr));
    let run = shale().arg("--export-pdf").arg(dir.join("Empty")).arg(dir.join("b.pdf")).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("no notes to export"));
    assert!(!dir.join("a.pdf").exists() && !dir.join("b.pdf").exists());
    // a wrong command line is 2, as for every option
    let run = shale().arg("--export-pdf").arg("only-a-folder").output().unwrap();
    assert_eq!(run.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: BOOK-011
#[cfg(unix)]
#[test]
fn userguide_pdf_script_writes_the_guide_as_a_pdf() {
    let out = std::env::temp_dir().join(format!("shale-guide-pdf-{}", std::process::id())).join("guide.pdf");
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
    let run = Command::new("sh").arg(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/userguide-pdf.sh")).arg(&out).env("SHALE", env!("CARGO_BIN_EXE_shale")).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), out.to_string_lossy(), "it says where the file is");
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}
