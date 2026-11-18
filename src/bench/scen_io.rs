//! Scenarios that read and write files and run other programs (BUILD-028): importing Word files
//! (pandoc), exporting PDFs (Shale's own writer, and the TeX path), rendering a PDF, and syncing
//! a vault with git and git-LFS. Whatever needs a program that is not installed is skipped with
//! the reason, never failed.

use super::content_gen as gen;
use super::scenarios::{Body, Fixtures, Prep, Scenario};
use super::vault_gen::Size;
use crate::gitsync::{self, Git, Options, TreeLock};
use crate::vault::Vault;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

// ---- import ----------------------------------------------------------------------------------

fn pandoc() -> Result<PathBuf, String> {
    crate::import::docx::locate("").ok_or_else(|| "pandoc is not installed (Settings → Import says how)".to_string())
}

/// The Word files of this run, written once.
pub fn docx_files(fx: &mut Fixtures) -> Result<Vec<PathBuf>, String> {
    let (n, kb) = (fx.scale.docx_files, fx.scale.docx_kb);
    let dir = fx.cached("docx", |dir| {
        for i in 0..n {
            std::fs::write(dir.join(format!("Report {}.docx", i + 1)), gen::docx(100 + i as u64, kb * 1024)).map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    Ok((0..n).map(|i| dir.join(format!("Report {}.docx", i + 1))).collect())
}

/// Import Word files into the vault at `root` the way the import dialog does; the notes' paths.
pub fn import_docx(root: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let pandoc = pandoc()?;
    let mut vault = Vault::open(root).map_err(|e| e.to_string())?;
    let cancel = AtomicBool::new(false);
    let mut reserved = std::collections::HashSet::new();
    let mut notes = vec![];
    for f in files {
        let c = crate::import::docx::Convert { pandoc: &pandoc, docx: f, root, date: (2026, 11, 18), keep_original: true, timeout: crate::import::docx::TIMEOUT, cancel: &cancel };
        let prepared = crate::import::docx::prepare(&c, &mut reserved)?;
        let text = crate::import::docx::note_text(&prepared, false, &[]);
        prepared.commit(root)?;
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        notes.push(vault.create_file(Path::new(&format!("{stem}.md")), &text).map_err(|e| e.to_string())?);
    }
    Ok(notes)
}

fn import_docx_big(fx: &mut Fixtures) -> Prep {
    pandoc()?;
    let files = docx_files(fx)?;
    let base = fx.scratch("docx-import");
    let mut run = 0;
    Ok(Box::new(move || {
        run += 1;
        let root = base.join(format!("run{run}"));
        std::fs::create_dir_all(&root).expect("a vault folder");
        black_box(import_docx(&root, &files).expect("the Word files are imported"));
    }))
}

/// A vault with the imported Word files as notes, shared by the scenarios that need it.
pub fn imported_vault(fx: &mut Fixtures) -> Result<PathBuf, String> {
    pandoc()?;
    let files = docx_files(fx)?;
    fx.cached("docx-vault", |dir| import_docx(dir, &files).map(|_| ()))
}

// ---- PDF -------------------------------------------------------------------------------------

fn pdf_export_note(fx: &mut Fixtures) -> Body {
    let g = fx.vault(Size::Large);
    let vault = super::scenarios::open(&g);
    let text = fx.long_text(Size::Large);
    Box::new(move || {
        black_box(crate::pdf::export::export_note(&vault, &g.long_note, &text, &crate::pdf::export::ExportOptions::for_note()).expect("the note exports"));
    })
}

fn book_of(vault: &Vault, dir: &str) -> Vec<u8> {
    crate::pdf::book_export::export_book(vault, Path::new(dir), &crate::pdf::export::ExportOptions::for_note(), None).expect("the book exports")
}

fn pdf_export_book(fx: &mut Fixtures) -> Body {
    let vault = super::scenarios::open(&fx.vault(Size::Large));
    Box::new(move || {
        black_box(book_of(&vault, "Area 00"));
    })
}

fn pdf_export_book_docx(fx: &mut Fixtures) -> Prep {
    let vault = Vault::open(&imported_vault(fx)?).map_err(|e| e.to_string())?;
    Ok(Box::new(move || {
        black_box(book_of(&vault, ""));
    }))
}

fn pdf_render(fx: &mut Fixtures) -> Body {
    let vault = super::scenarios::open(&fx.vault(Size::Large));
    let pdf = book_of(&vault, "Area 01");
    Box::new(move || {
        black_box(crate::pdf::view::render_pages(pdf.clone(), 6, 1.0).expect("the pages render"));
    })
}

fn tex_blocks(fx: &mut Fixtures) -> Body {
    let text = fx.long_text(Size::Large);
    Box::new(move || {
        black_box(crate::pdf::tex::blocks::parse(&text));
        black_box(crate::pdf::tex::blocks::footnote_texts(&text));
    })
}

/// The engine run of the TeX export: the program's time, not Shale's. It needs a TeX.
fn tex_engine(_fx: &mut Fixtures) -> Prep {
    let found = crate::pdf::tex::engine::detect(None, std::env::var_os("PATH").as_deref());
    let engine = found.selected(crate::pdf::tex::engine::TexChoice::System).map_err(|e| format!("no TeX for the engine run: {e}"))?.clone();
    Ok(Box::new(move || {
        black_box(crate::pdf::tex::compile::compile_test_document(&engine).expect("the test document compiles"));
    }))
}

// ---- git -------------------------------------------------------------------------------------

/// A git that reads no configuration of the person's, with the branch `main`. Git-LFS is used if
/// the computer has it (a vault without media is not touched by it).
fn hermetic() -> Git {
    let env = [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_COUNT", "3"),
        ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
        ("GIT_CONFIG_VALUE_0", "main"),
        ("GIT_CONFIG_KEY_1", "protocol.file.allow"),
        ("GIT_CONFIG_VALUE_1", "always"),
        ("GIT_CONFIG_KEY_2", "core.autocrlf"),
        ("GIT_CONFIG_VALUE_2", "false"),
    ];
    Git { program: "git".into(), env: env.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect() }
}

fn git_available() -> Result<(), String> {
    Git::default().version().map(|_| ()).ok_or_else(|| "git is not installed".to_string())
}

fn bare_remote(dir: &Path) -> PathBuf {
    let p = dir.join("remote.git");
    std::fs::create_dir_all(&p).expect("a folder for the remote");
    let ok = Command::new("git").args(["init", "--quiet", "--bare"]).arg(&p).envs([("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")]).status().is_ok_and(|s| s.success());
    assert!(ok, "git init --bare failed");
    p
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder");
    for e in std::fs::read_dir(from).expect("a folder to copy").flatten() {
        let t = to.join(e.file_name());
        if e.file_type().is_ok_and(|k| k.is_dir()) {
            copy_tree(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), &t).expect("a file is copied");
        }
    }
}

/// Make the folder a git repository with a bare remote beside it, as the sync settings would.
pub fn connect_repository(vault: &Path) -> Result<(), String> {
    git_available()?;
    let remote = bare_remote(vault.parent().ok_or("no parent folder")?);
    gitsync::connect(&hermetic(), vault, remote.to_str().ok_or("a path that is no text")?)
}

fn sync_options() -> Options {
    Options { commit_prefix: String::new(), commit_suffix: String::new(), host: "bench".into(), stamp: "2026-11-18 1200".into() }
}

fn sync(git: &Git, vault: &Path) -> gitsync::Report {
    gitsync::sync(git, vault, &sync_options(), &TreeLock::default()).expect("the sync works")
}

/// A copy of the large vault, connected to a new bare remote (`sync` of it commits everything).
fn git_vault(fx: &mut Fixtures, git: &Git, size: Size) -> (PathBuf, PathBuf) {
    let g = fx.vault(size);
    let dir = fx.scratch("git");
    let (vault, remote) = (dir.join("vault"), bare_remote(&dir));
    copy_tree(&g.root, &vault);
    gitsync::connect(git, &vault, remote.to_str().expect("a path")).expect("the vault connects");
    (vault, remote)
}

fn git_initial(fx: &mut Fixtures) -> Prep {
    git_available()?;
    let git = hermetic();
    let (vault, _remote) = git_vault(fx, &git, Size::Large);
    Ok(Box::new(move || {
        black_box(sync(&git, &vault));
    }))
}

fn git_incremental(fx: &mut Fixtures) -> Prep {
    git_available()?;
    let git = hermetic();
    let (vault, _remote) = git_vault(fx, &git, Size::Large);
    sync(&git, &vault);
    let mut areas: Vec<PathBuf> = std::fs::read_dir(&vault).expect("the vault").flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Area "))).collect();
    areas.sort();
    let mut round = 0;
    Ok(Box::new(move || {
        round += 1;
        for i in 0..50 {
            let note = std::fs::read_dir(&areas[i % areas.len()]).expect("an area").flatten().find(|e| e.path().extension().is_some_and(|x| x == "md")).expect("a note").path();
            let mut text = std::fs::read_to_string(&note).expect("a note is read");
            text.push_str(&format!("\nEdit {round}.\n"));
            std::fs::write(&note, text).expect("a note is written");
        }
        black_box(sync(&git, &vault));
    }))
}

fn git_pull(fx: &mut Fixtures) -> Prep {
    git_available()?;
    let git = hermetic();
    let (vault, remote) = git_vault(fx, &git, Size::Large);
    sync(&git, &vault);
    let other = fx.scratch("git-other").join("vault");
    std::fs::create_dir_all(&other).expect("a folder");
    gitsync::connect(&git, &other, remote.to_str().expect("a path")).expect("the second vault connects");
    Ok(Box::new(move || {
        black_box(sync(&git, &other));
    }))
}

fn git_lfs(fx: &mut Fixtures) -> Prep {
    git_available()?;
    if Git::default().lfs_version().is_none() {
        return Err("git-lfs is not installed".into());
    }
    let git = hermetic();
    let (vault, _remote) = git_vault(fx, &git, Size::Small);
    std::fs::create_dir_all(vault.join("Attachments")).map_err(|e| e.to_string())?;
    for i in 0..fx.scale.lfs_files {
        let mut r = super::vault_gen::Rng::new(77 + i as u64);
        let bytes: Vec<u8> = (0..fx.scale.lfs_kb * 1024).map(|_| r.next() as u8).collect();
        std::fs::write(vault.join(format!("Attachments/photo {i}.png")), bytes).map_err(|e| e.to_string())?;
    }
    Ok(Box::new(move || {
        black_box(sync(&git, &vault));
    }))
}

pub fn list() -> Vec<Scenario> {
    let (p, m) = (Scenario::plain, Scenario::maybe);
    vec![
        m("import.docx.big", 2, false, import_docx_big),
        p("pdf.export.note.long", 5, true, pdf_export_note),
        p("pdf.export.book.large", 3, true, pdf_export_book),
        m("pdf.export.book.docx", 3, false, pdf_export_book_docx),
        p("pdf.render.book", 3, false, pdf_render),
        p("pdf.tex.blocks.long", 20, false, tex_blocks),
        m("pdf.tex.engine", 2, false, tex_engine),
        m("git.sync.initial.large", 1, false, git_initial).cold(),
        m("git.sync.incremental.large", 5, true, git_incremental),
        m("git.sync.pull.large", 1, false, git_pull).cold(),
        m("git.lfs.sync.media", 1, false, git_lfs).cold(),
    ]
}
