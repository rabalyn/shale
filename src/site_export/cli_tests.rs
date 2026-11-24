//! `shale export-site` (SITE-009, SITE-011…SITE-015): the command line, the dry run, the atomic
//! write and the manifest. The program itself is run by `tests/export_site_cli.rs`.

use super::cli::{export, parse, Args};
use super::plan::{Content, OutFile};
use super::test_support::{snapshot, Fixture};
use super::write::{write, MANIFEST};
use std::path::{Path, PathBuf};

fn args(f: &Fixture, dry_run: bool) -> Args {
    Args { vault: f.dir.clone(), out: f.out(), dry_run }
}

fn run(a: &Args) -> (Result<(), String>, Vec<String>, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let r = export(a, &[], &mut out, &mut err);
    (r, String::from_utf8(out).unwrap().lines().map(String::from).collect(), String::from_utf8(err).unwrap())
}

fn vault() -> Fixture {
    let f = Fixture::with("cli", "\"all_notes\": true", &[("A.md", "see [[B]] and ![[p.png]] and [[Closed]]\n"), ("B.md", "b\n"), ("Folder/C.md", "c\n")]);
    f.file("Closed.md", "---\npublish: false\n---\nclosed\n");
    f.bytes("p.png", b"png");
    f.bytes("unused.png", b"png");
    f
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn tree(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = snapshot(dir).into_iter().map(|(p, _, _)| p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
    v.sort();
    v
}

// spec: SITE-012
#[test]
fn site_012_the_command_line_takes_exactly_a_vault_folder_an_output_folder_and_dry_run() {
    assert_eq!(parse(strings(&["v", "o"])).unwrap(), Args { vault: "v".into(), out: "o".into(), dry_run: false });
    assert!(parse(strings(&["--dry-run", "v", "o"])).unwrap().dry_run);
    assert!(parse(strings(&["v", "o", "--dry-run"])).unwrap().dry_run);
    assert!(parse(strings(&["v"])).unwrap_err().contains("needed"));
    assert!(parse(strings(&["v", "o", "x"])).unwrap_err().contains("3 folder names"));
    assert!(parse(strings(&["v", "o", "--publish"])).unwrap_err().contains("unknown option --publish"));
    assert!(parse(Vec::new()).is_err());
    // a folder that is not there, or an output folder inside the vault: the reason, and nothing is written
    let f = vault();
    let (r, _, _) = run(&Args { vault: f.root.join("nothing"), out: f.out(), dry_run: false });
    assert!(r.unwrap_err().contains("is not a folder"));
    assert!(!f.root.join("nothing").exists(), "a missing vault folder is not made");
    let inside = f.dir.join("site");
    let (r, _, _) = run(&Args { vault: f.dir.clone(), out: inside.clone(), dry_run: false });
    assert!(r.unwrap_err().contains("in the vault"));
    assert!(!inside.exists());
    let (r, _, _) = run(&Args { vault: f.dir.clone(), out: f.dir.clone(), dry_run: true });
    assert!(r.is_err());
}

// spec: SITE-009
#[test]
fn site_009_a_dry_run_lists_every_note_and_media_file_and_writes_nothing() {
    let f = vault();
    let before = snapshot(&f.root);
    let (r, lines, _) = run(&args(&f, true));
    r.unwrap();
    assert_eq!(&lines[..4], ["note A.md -> content/a.md", "note B.md -> content/b.md", "note Folder/C.md -> content/folder/c.md", "media static/media/files/p.png"]);
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[4].starts_with("Would write 3 notes, 1 media file and 1 link; 1 link became plain text"), "{}", lines[4]);
    assert_eq!(before, snapshot(&f.root), "nothing was written, not even a folder");
    assert!(!f.out().exists());
}

// spec: SITE-011, SITE-015
#[test]
fn site_011_the_command_writes_the_tree_and_the_last_line_is_the_summary() {
    let f = vault();
    let (r, lines, err) = run(&args(&f, false));
    r.unwrap();
    assert_eq!(err, "");
    assert_eq!(lines.last().unwrap(), "Wrote 3 notes, 1 media file and 1 link; 1 link became plain text.");
    let out = f.out();
    for path in ["hugo.toml", "content/a.md", "content/b.md", "content/folder/c.md", "content/folder/_index.md", "content/_index.md", "static/media/files/p.png", "data/backlinks.json", "data/graph.json", "themes/shale/theme.toml", MANIFEST]
    {
        assert!(out.join(path).is_file(), "{path}: {:?}", tree(&out));
    }
    assert_eq!(std::fs::read(out.join("static/media/files/p.png")).unwrap(), b"png");
    assert!(!out.join("static/media/files/unused.png").exists());
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(out.join(MANIFEST)).unwrap()).unwrap();
    let listed: Vec<String> = manifest["files"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let mut on_disk = tree(&out);
    on_disk.retain(|p| p != MANIFEST);
    assert_eq!(listed, on_disk, "the manifest lists every file the export wrote, and nothing else");
    assert!(tree(&f.root).iter().all(|p| !p.contains("shale-tmp")), "no work folder is left");
    // the vault is as it was
    assert!(!f.dir.join("site").exists() && !f.dir.join(".shale/state.json").exists());
}

// spec: SITE-015
#[test]
fn site_015_the_summary_counts_with_singular_and_plural_and_warnings_go_to_stderr() {
    let f = Fixture::with("s015", "\"all_notes\": true", &[("A.md", "![[@Nowhere/x.png]]\n")]);
    let (r, lines, err) = run(&args(&f, true));
    r.unwrap();
    assert_eq!(lines.last().unwrap(), "Would write 1 note, 0 media files and 0 links; 1 link became plain text.");
    assert!(err.starts_with("shale: warning: A.md: @Nowhere/x.png is not published"), "{err}");
    let empty = Fixture::new("s015b");
    let (r, lines, _) = run(&Args { vault: empty.dir.clone(), out: empty.out(), dry_run: false });
    r.unwrap();
    assert!(lines[0].starts_with("Publishing is off for this vault"), "{lines:?}");
    assert_eq!(lines.last().unwrap(), "Wrote 0 notes, 0 media files and 0 links; 0 links became plain text.");
    assert!(empty.out().join("hugo.toml").is_file() && empty.out().join("content/_index.md").is_file() && std::fs::read_dir(empty.out().join("content")).unwrap().count() == 1);
    // the dry run lists the generated page
    let (_, lines, _) = run(&Args { vault: empty.dir.clone(), out: empty.out(), dry_run: true });
    assert!(lines.contains(&"page content/_index.md".to_string()), "{lines:?}");
}

// spec: SITE-014
#[test]
fn site_014_only_the_files_of_an_earlier_export_are_replaced_or_removed() {
    let f = vault();
    run(&args(&f, false)).0.unwrap();
    let out = f.out();
    std::fs::write(out.join("README.txt"), "mine").unwrap();
    std::fs::create_dir_all(out.join("extra")).unwrap();
    std::fs::write(out.join("extra/own.md"), "mine too").unwrap();
    // B and the picture are no longer published; the rest is written again
    f.file("B.md", "---\npublish: false\n---\nb\n");
    f.file("A.md", "changed [[Folder/C]]\n");
    run(&args(&f, false)).0.unwrap();
    assert_eq!(std::fs::read_to_string(out.join("README.txt")).unwrap(), "mine");
    assert_eq!(std::fs::read_to_string(out.join("extra/own.md")).unwrap(), "mine too");
    assert!(!out.join("content/b.md").exists() && !out.join("static").exists(), "what the last export wrote and this one does not is gone, also its empty folders: {:?}", tree(&out));
    assert!(std::fs::read_to_string(out.join("content/a.md")).unwrap().contains("changed"));
    // a file at a place of the site that no export wrote is never replaced: the export stops
    std::fs::remove_file(out.join(MANIFEST)).unwrap();
    let before = snapshot(&out);
    let (r, _, _) = run(&args(&f, false));
    assert!(r.unwrap_err().contains("is not a file of an earlier export"));
    assert_eq!(before, snapshot(&out), "nothing was written");
}

// spec: SITE-013
#[test]
fn site_013_a_failed_run_leaves_the_output_folder_as_it_was() {
    let f = vault();
    let first = super::plan::build(&crate::vault::Vault::open(&f.dir).unwrap()).unwrap();
    let out = f.out();
    write(&first, &out).unwrap();
    let before = snapshot(&out);
    // the second tree cannot be completed: a file to copy is not there
    let mut second = super::plan::build(&crate::vault::Vault::open(&f.dir).unwrap()).unwrap();
    second.files.push(OutFile { path: "static/media/files/gone.png".into(), content: Content::Copy(PathBuf::from("/nowhere/at/all.png")) });
    second.files.push(OutFile { path: "content/zzz.md".into(), content: Content::Text("late".into()) });
    let e = write(&second, &out).unwrap_err();
    assert!(e.contains("could not copy"), "{e}");
    assert_eq!(before, snapshot(&out), "as it was");
    assert!(tree(&f.root).iter().all(|p| !p.contains("shale-tmp")), "the work folder is gone: {:?}", tree(&f.root));
    // a path that leaves the site is refused before anything is made
    let mut bad = super::plan::build(&crate::vault::Vault::open(&f.dir).unwrap()).unwrap();
    bad.files.push(OutFile { path: "../escape.md".into(), content: Content::Text("x".into()) });
    assert!(write(&bad, &out).unwrap_err().contains("not a path inside the site"));
    assert!(!f.root.join("escape.md").exists());
    // a first export that fails leaves no output folder
    let fresh = f.root.join("fresh");
    assert!(write(&second, &fresh).is_err());
    assert!(!fresh.exists());
}
