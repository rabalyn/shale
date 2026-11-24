//! `shale export-site <vault> <out> [--dry-run]` (SITE-009, SITE-011…SITE-016): the site of a
//! vault, written without a window, like `--export-pdf` (BOOK-011); and `shale export-site
//! --publish <vault> [--take-over]` (SITE-066, SITE-076), which publishes it.

use super::{plan, publish, write};
use crate::busy::Cancel;
use crate::gitsync::{credentials, managed, Git};
use crate::media::outside::Library;
use crate::vault::Vault;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: shale export-site <vault folder> <output folder> [--dry-run]\n       shale export-site --publish <vault folder> [--take-over]";

/// What the command line of `export-site` says (SITE-012).
#[derive(Debug, PartialEq)]
pub struct Args {
    pub vault: PathBuf,
    pub out: PathBuf,
    pub dry_run: bool,
}

/// Read the arguments after `export-site`: exactly a vault folder and an output folder, and
/// `--dry-run` anywhere. Anything else is an error that says why.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let (mut dry_run, mut paths) = (false, Vec::new());
    for a in args {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            o if o.starts_with("--") => return Err(format!("unknown option {o}")),
            _ => paths.push(PathBuf::from(a)),
        }
    }
    match <[PathBuf; 2]>::try_from(paths) {
        Ok([vault, out]) => Ok(Args { vault, out, dry_run }),
        Err(paths) => Err(format!("a vault folder and an output folder are needed, not {} folder names", paths.len())),
    }
}

/// `path` made absolute with its existing part resolved (links, `..`), also when it does not exist yet.
fn resolved(path: &Path) -> PathBuf {
    let Ok(abs) = std::path::absolute(path) else { return path.to_path_buf() };
    let (mut head, mut tail) = (abs.clone(), Vec::new());
    loop {
        if let Ok(real) = head.canonicalize() {
            return tail.iter().rev().fold(real, |p, part| p.join(part));
        }
        match head.file_name().map(|n| n.to_os_string()) {
            Some(name) => {
                tail.push(name);
                head.pop();
            }
            None => return abs,
        }
    }
}

pub(super) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The summary line (SITE-015): what was written, and how many links became plain text.
pub fn summary(e: &plan::Export, dry_run: bool) -> String {
    counts(if dry_run { "Would write" } else { "Wrote" }, e.notes.len(), e.media.len(), e.links, e.plain)
}

/// "Wrote 2 notes, 0 media files and 1 link; 1 link became plain text." for the verb and the counts.
pub(super) fn counts(verb: &str, notes: usize, media: usize, links: usize, plain: usize) -> String {
    format!("{verb} {}, {} and {}; {} became plain text.", plural(notes, "note", "notes"), plural(media, "media file", "media files"), plural(links, "link", "links"), plural(plain, "link", "links"),)
}

/// Run the export: the plan, then the listing or the files. Says what it did on `out`, the warnings on `err`.
pub fn export(args: &Args, libraries: &[Library], out: &mut dyn Write, err: &mut dyn Write) -> Result<(), String> {
    if !args.vault.is_dir() {
        return Err(format!("{} is not a folder", args.vault.display()));
    }
    let (vault_dir, target) = (resolved(&args.vault), resolved(&args.out));
    if target.starts_with(&vault_dir) {
        return Err(format!("the output folder {} is in the vault, which an export never changes", args.out.display()));
    }
    let mut vault = Vault::open(&vault_dir).map_err(|e| format!("could not open {}: {e}", args.vault.display()))?;
    vault.set_libraries(libraries);
    let export = plan::build(&vault)?;
    for w in &export.warnings {
        let _ = writeln!(err, "shale: warning: {w}");
    }
    if let Some(why) = &export.empty_because {
        let _ = writeln!(out, "{why}.");
    }
    if args.dry_run {
        for (note, content) in &export.notes {
            let _ = writeln!(out, "note {note} -> content/{content}");
        }
        for media in &export.media {
            let _ = writeln!(out, "media {media}");
        }
        // a site without a note has the generated hello-world page (SITE-001)
        if export.notes.is_empty() {
            for f in export.files.iter().filter(|f| f.path.starts_with("content/")) {
                let _ = writeln!(out, "page {}", f.path);
            }
        }
    } else {
        write::write(&export, &args.out)?;
    }
    let _ = writeln!(out, "{}", summary(&export, args.dry_run));
    Ok(())
}

/// The libraries of this device (MEDIA-014), read from the global settings without changing them.
fn device_libraries() -> Vec<Library> {
    crate::config::config_path().map(|p| crate::config::Config::load_from(&p).0.media_libraries).unwrap_or_default()
}

/// What `--publish` says (SITE-066): the vault, and whether it takes over a target that another
/// vault published to (SITE-074).
#[derive(Debug, PartialEq)]
pub struct PublishArgs {
    pub vault: PathBuf,
    pub take_over: bool,
}

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
    Export(Args),
    Publish(PublishArgs),
}

/// Read the arguments after `export-site`: an export, or with `--publish` exactly one vault folder
/// and `--take-over` anywhere.
pub fn parse_command(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let args: Vec<String> = args.into_iter().collect();
    if !args.iter().any(|a| a == "--publish") {
        return parse(args).map(Command::Export);
    }
    let (mut take_over, mut paths) = (false, Vec::new());
    for a in args {
        match a.as_str() {
            "--publish" => {}
            "--take-over" => take_over = true,
            o if o.starts_with("--") => return Err(format!("unknown option {o}")),
            _ => paths.push(PathBuf::from(a)),
        }
    }
    match <[PathBuf; 1]>::try_from(paths) {
        Ok([vault]) => Ok(Command::Publish(PublishArgs { vault, take_over })),
        Err(paths) => Err(format!("--publish takes one vault folder, not {} folder names", paths.len())),
    }
}

/// Publish the vault (SITE-066): the progress and the warnings on `err`, the summary on `out`.
pub fn publish(args: &PublishArgs, git: &Git, libraries: &[Library], base: &Path, out: &mut dyn Write, err: &mut dyn Write) -> Result<(), String> {
    if !args.vault.is_dir() {
        return Err(format!("{} is not a folder", args.vault.display()));
    }
    let vault = resolved(&args.vault);
    let request = publish::Request { vault: &vault, libraries, base, device: &crate::gitsync::host_name(), take_over: args.take_over };
    let err = std::cell::RefCell::new(err);
    let step = |s: &str| {
        let _ = writeln!(err.borrow_mut(), "shale: {s}");
    };
    let done = publish::publish(git, &request, &Cancel::default(), &step).map_err(|f| f.words())?;
    let err = err.into_inner();
    for w in &done.summary.warnings {
        let _ = writeln!(err, "shale: warning: {w}");
    }
    if let Some(why) = &done.summary.empty_because {
        let _ = writeln!(out, "{why}.");
    }
    let _ = writeln!(out, "{}", done.summary.line());
    Ok(())
}

/// The git of this device as the window runs it (SYNC-024) with Shale as its askpass helper (SYNC-054).
pub(super) fn device_git(choice: managed::GitChoice) -> Git {
    let mut git = managed::selected_git(choice, dirs::data_dir().as_deref(), std::env::var_os("PATH").as_deref());
    if let Ok(exe) = std::env::current_exe() {
        credentials::attach(&mut git, &exe);
    }
    git
}

/// `shale export-site …` with the arguments after the word `export-site`: the exit code, 0 or 1.
pub fn run(args: impl IntoIterator<Item = String>) -> i32 {
    let result = parse_command(args).and_then(|c| match c {
        Command::Export(a) => export(&a, &device_libraries(), &mut std::io::stdout(), &mut std::io::stderr()),
        Command::Publish(a) => {
            let cfg = crate::config::config_path().map(|p| crate::config::Config::load_from(&p).0).unwrap_or_default();
            publish(&a, &device_git(cfg.git.choice), &cfg.media_libraries, &crate::device_local::default_base(), &mut std::io::stdout(), &mut std::io::stderr())
        }
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("shale: {e}\n{USAGE}");
            1
        }
    }
}
