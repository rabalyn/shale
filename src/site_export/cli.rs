//! `shale export-site <vault> <out> [--dry-run]` (SITE-009, SITE-011…SITE-016): the site of a
//! vault, written without a window, like `--export-pdf` (BOOK-011).

use super::{plan, write};
use crate::media::outside::Library;
use crate::vault::Vault;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: shale export-site <vault folder> <output folder> [--dry-run]";

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

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The summary line (SITE-015): what was written, and how many links became plain text.
pub fn summary(e: &plan::Export, dry_run: bool) -> String {
    format!(
        "{} {}, {} and {}; {} became plain text.",
        if dry_run { "Would write" } else { "Wrote" },
        plural(e.notes.len(), "note", "notes"),
        plural(e.media.len(), "media file", "media files"),
        plural(e.links, "link", "links"),
        plural(e.plain, "link", "links"),
    )
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

/// `shale export-site …` with the arguments after the word `export-site`: the exit code, 0 or 1.
pub fn run(args: impl IntoIterator<Item = String>) -> i32 {
    let result = parse(args).and_then(|a| export(&a, &device_libraries(), &mut std::io::stdout(), &mut std::io::stderr()));
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("shale: {e}\n{USAGE}");
            1
        }
    }
}
