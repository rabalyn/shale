//! Writing the site (SITE-013, SITE-014): the tree is made next to the output folder and moved in
//! only when it is complete, and only the files of an earlier export (named in its manifest) are
//! replaced or removed, so whatever else the folder holds stays.

use super::plan::{Content, Export};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// The list of the files an export wrote, in the output folder (SITE-014).
pub const MANIFEST: &str = ".shale-export.json";

/// The files the last export wrote to `out`; none when it has no manifest or the manifest cannot be read.
fn earlier_files(out: &Path) -> BTreeSet<String> {
    let Ok(bytes) = crate::fsutil::read(out.join(MANIFEST)) else { return BTreeSet::new() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return BTreeSet::new() };
    v["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A path of the tree is a plain relative path: nothing in it can leave the output folder.
fn plain(path: &str) -> bool {
    Path::new(path).components().all(|c| matches!(c, Component::Normal(_))) && !path.is_empty()
}

fn io_error(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("could not {what} {}: {e}", path.display())
}

/// The folder the tree is made in, next to `out` (SITE-013).
fn work_folder(out: &Path) -> PathBuf {
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "site".into());
    out.with_file_name(format!(".{name}.shale-tmp-{}", std::process::id()))
}

fn remove_empty_parents(out: &Path, mut path: PathBuf) {
    while path.pop() && path != out && fs::remove_dir(&path).is_ok() {}
}

/// Write `export` to `out`. On an error `out` is as it was.
pub fn write(export: &Export, out: &Path) -> Result<(), String> {
    if let Some(bad) = export.files.iter().find(|f| !plain(&f.path)) {
        return Err(format!("{} is not a path inside the site", bad.path));
    }
    if out.exists() && !out.is_dir() {
        return Err(format!("{} is not a folder", out.display()));
    }
    let earlier = earlier_files(out);
    // a file that an earlier export did not write is never replaced (SITE-014)
    for f in &export.files {
        let target = out.join(&f.path);
        if target.symlink_metadata().is_ok() && !earlier.contains(&f.path) {
            return Err(format!("{} is not a file of an earlier export and is kept; nothing was written", target.display()));
        }
    }
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| io_error("create", parent, e))?;
    let work = work_folder(out);
    if work.exists() {
        let _ = fs::remove_dir_all(&work);
    }
    let made = make_tree(export, &work);
    if let Err(e) = made.and_then(|()| move_in(export, &work, out, &earlier)) {
        let _ = fs::remove_dir_all(&work);
        return Err(e);
    }
    let _ = fs::remove_dir_all(&work);
    Ok(())
}

/// The whole tree, and the manifest, in `work`.
fn make_tree(export: &Export, work: &Path) -> Result<(), String> {
    fs::create_dir_all(work).map_err(|e| io_error("create", work, e))?;
    for f in &export.files {
        let target = work.join(&f.path);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error("create", dir, e))?;
        }
        match &f.content {
            Content::Text(text) => crate::fsutil::write_atomic(&target, text.as_bytes()).map_err(|e| io_error("write", &target, e))?,
            Content::Copy(from) => crate::fsutil::copy_new(from, &target).map_err(|e| io_error("copy", from, e))?,
        }
    }
    let files: Vec<&str> = export.files.iter().map(|f| f.path.as_str()).collect();
    let manifest = format!("{}\n", serde_json::to_string_pretty(&json!({"version": 1, "files": files})).unwrap_or_default());
    crate::fsutil::write_atomic(&work.join(MANIFEST), manifest.as_bytes()).map_err(|e| io_error("write", &work.join(MANIFEST), e))
}

/// The finished tree into `out`: its files replace those of the earlier export, the files of the
/// earlier export that are not in this one go, the manifest comes last.
fn move_in(export: &Export, work: &Path, out: &Path, earlier: &BTreeSet<String>) -> Result<(), String> {
    if !out.exists() {
        return crate::fsutil::rename_noreplace(work, out).map_err(|e| io_error("move the site to", out, e));
    }
    let mut paths: Vec<&str> = export.files.iter().map(|f| f.path.as_str()).collect();
    paths.push(MANIFEST);
    for path in &paths {
        let target = out.join(path);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).map_err(|e| io_error("create", dir, e))?;
        }
        crate::fsutil::rename_replace(&work.join(path), &target).map_err(|e| io_error("write", &target, e))?;
    }
    let now: BTreeSet<&str> = paths.into_iter().collect();
    for gone in earlier.iter().filter(|p| !now.contains(p.as_str()) && plain(p)) {
        let target = out.join(gone);
        if fs::remove_file(&target).is_ok() {
            remove_empty_parents(out, target);
        }
    }
    Ok(())
}
