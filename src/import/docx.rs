//! Word files (.docx) become notes through pandoc (IMP-035 to IMP-042). Pandoc is a program of the
//! user's (BUILD-018), run like the other programs: an argument list, no shell, stdin closed, no
//! console window on Windows, a time limit. Shale's Lua filter (`assets/import/shale-docx.lua`)
//! shapes its Markdown. Nothing reaches the vault before pandoc has ended with success: the
//! pictures are extracted into a hidden folder next to the import folder, which is then renamed
//! into place; a run that fails or is cancelled leaves nothing behind.

use super::{docx_look, front, yaml, Item, Options, Source};
use crate::pdf::tex::proc::{self, Ending};
use crate::vault::{rel_to_string, Vault};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The filter, built into the program and written to a temp file for each run (IMP-037).
const FILTER: &str = include_str!("../../assets/import/shale-docx.lua");

/// How long pandoc may run on one file before it is killed (IMP-035).
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// The longest name of a folder Shale makes for an import: half of the 255 bytes of ext4 and APFS (VAULT-026).
const MAX_NAME: usize = 127;

/// The start of the name of an import folder.
const PREFIX: &str = "import_docx_";

/// What the dialog says about a Word file when pandoc is not there.
pub const MISSING: &str = "pandoc was not found (Settings → Import)";

/// How to install pandoc, for each system (IMP-036).
pub const INSTALL_HELP: [(&str, &str); 3] = [("Linux", "sudo pacman -S pandoc-cli   or   sudo apt install pandoc"), ("macOS", "brew install pandoc"), ("Windows", "winget install JohnMacFarlane.Pandoc")];

/// Where pandoc's installers put it on Windows, where a window program's `PATH` may lack it.
fn windows_dirs() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles"].iter().filter_map(std::env::var_os).map(|d| PathBuf::from(d).join("Pandoc")).collect()
}

/// The pandoc to run: the program the user chose (`chosen`, empty: none), else the first in `PATH` and the usual places.
pub fn locate(chosen: &str) -> Option<PathBuf> {
    crate::program::locate_chosen(chosen, "pandoc", &crate::program::search_dirs(&windows_dirs()))
}

/// The first line of `pandoc --version` ("pandoc 3.14"), or why it could not be had.
pub fn version(pandoc: &Path) -> Result<String, String> {
    let run = crate::pandoc_limit::run(
        pandoc,
        |cmd| {
            cmd.arg("--version");
        },
        |cmd| proc::run(cmd, Duration::from_secs(20), None, |_| {}),
    )
    .map_err(|e| format!("could not start {}: {e}", pandoc.display()))?;
    match run.tail.iter().find(|l| !l.trim().is_empty()) {
        Some(line) if run.success() => Ok(line.trim().to_string()),
        Some(line) => Err(line.trim().to_string()),
        None => Err("it printed nothing".into()),
    }
}

/// An item for a Word file. Without a `pandoc` the file is saved as an attachment, and the item says why.
pub fn docx_item(path: PathBuf, pandoc: Option<PathBuf>) -> Item {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
    Item { source: Source::Docx { name, path, pandoc, lift: true, ready: None }, include: true, title: super::safe_title(&stem), att_on: vec![], already: None, people: vec![] }
}

/// The name of the folder for the Word file `stem` (IMP-038): `import_docx_<name>`, the name with
/// `-` for spaces, link-safe, portable, runs of `-` made one, `-` trimmed, `document` when nothing
/// is left, and a number (`-2`, `-3`, …) while `taken` says the name is. At most 127 bytes.
pub fn import_folder_name(stem: &str, taken: impl Fn(&str) -> bool) -> String {
    let dashed = crate::markdown::link_safe_name(&stem.replace(' ', "-"));
    let mut name = String::new();
    for c in crate::fsutil::portable_name(&dashed, MAX_NAME).chars() {
        if !(c == '-' && name.ends_with('-')) {
            name.push(c);
        }
    }
    let name = name.trim_matches('-').trim_end_matches(['.', ' ']);
    let name = if name.is_empty() { "document" } else { name };
    for n in 1.. {
        let suffix = if n == 1 { String::new() } else { format!("-{n}") };
        // the number is part of the 127 bytes
        let base = crate::fsutil::portable_name(name, MAX_NAME - PREFIX.len() - suffix.len());
        let base = base.trim_end_matches(['-', '.', ' ']);
        let folder = format!("{PREFIX}{}{suffix}", if base.is_empty() { "document" } else { base });
        if !taken(&folder) {
            return folder;
        }
    }
    unreachable!("the loop ends with a free name")
}

/// The words pandoc is started with (IMP-035): the Word file, the filter, where its folder will be
/// (`media` is the vault path of the import folder, with `/`), and the note to write. It runs in the
/// folder the pictures go to, so `--extract-media=.` puts them in `media/` there.
pub fn args(docx: &Path, filter: &Path, media: &str, out: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![docx.into()];
    a.extend(["-f", "docx", "-t", "gfm", "--wrap=preserve", "--extract-media=.", "-M"].map(OsString::from));
    a.push(format!("shale-media={media}").into());
    let mut f = OsString::from("--lua-filter=");
    f.push(filter);
    a.push(f);
    a.push("-o".into());
    a.push(out.into());
    a
}

/// A run of pandoc on one Word file that has ended with success: the note's text, and the files
/// that go with it in a hidden folder waiting to be renamed into place by [`Prepared::commit`].
/// A value that is dropped without a commit removes that folder.
#[derive(Debug)]
pub struct Prepared {
    /// pandoc's Markdown
    pub text: String,
    /// the look of the file as properties (`print-*`, IMP-040)
    pub look: Vec<(&'static str, String)>,
    /// the import folder (vault-relative), when there is a file for it
    pub folder: Option<PathBuf>,
    /// the kept original inside it
    pub original: Option<PathBuf>,
    /// the hidden folder: its `files` become `folder`
    temp: Mutex<Option<PathBuf>>,
}

impl Prepared {
    /// Move the files into the vault. They are in place whole or not at all.
    pub fn commit(&self, root: &Path) -> Result<(), String> {
        let Some(temp) = self.temp.lock().unwrap_or_else(|e| e.into_inner()).take() else { return Ok(()) };
        let moved = match &self.folder {
            Some(f) => crate::fsutil::rename_noreplace(&temp.join("files"), &root.join(f)).map_err(|e| format!("could not make the folder “{}”: {e}", rel_to_string(f))),
            None => Ok(()),
        };
        remove_temp(&temp);
        moved
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(temp) = self.temp.lock().unwrap_or_else(|e| e.into_inner()).take() {
            remove_temp(&temp);
        }
    }
}

/// Remove the hidden folder, and the month, year and media folders above it when that leaves them
/// empty, so that an import that makes no folder leaves no trace.
fn remove_temp(temp: &Path) {
    let _ = std::fs::remove_dir_all(temp);
    let mut up = temp.parent();
    // the month, the year and the media folder (`remove_dir` takes only empty ones)
    for _ in 0..3 {
        let Some(dir) = up else { return };
        if std::fs::remove_dir(dir).is_err() {
            return;
        }
        up = dir.parent();
    }
}

/// Removes the hidden folder unless it was handed on.
struct TempFolder(Option<PathBuf>);

impl TempFolder {
    fn release(mut self) -> PathBuf {
        self.0.take().expect("released once")
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            remove_temp(&p);
        }
    }
}

/// What one run needs.
pub struct Convert<'a> {
    pub pandoc: &'a Path,
    pub docx: &'a Path,
    /// the vault folder
    pub root: &'a Path,
    /// the date of the import (local), which names the month folder
    pub date: (i32, u32, u32),
    pub keep_original: bool,
    pub timeout: Duration,
    /// set to stop the run (the dialog was closed)
    pub cancel: &'a AtomicBool,
}

/// Run pandoc on `c.docx` (IMP-035, IMP-038). `reserved` holds the folder names that other files of
/// the same import have chosen; the name chosen here is added to it. Nothing is left in the vault
/// when it fails.
pub fn prepare(c: &Convert, reserved: &mut HashSet<String>) -> Result<Prepared, String> {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let file = c.docx.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = c.docx.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // the dataflow of a Word import (DEV-030): the hidden folder, the look of the file, pandoc, the original
    let folder_step = tracing::info_span!("flow.import_docx.prepare_folder", peer = "vault").entered();
    let (y, m, _) = c.date;
    let month = PathBuf::from(crate::media::MEDIA_DIR).join(format!("{y:04}")).join(format!("{m:02}"));
    let month_abs = c.root.join(&month);
    let name = import_folder_name(&stem, |n| reserved.contains(n) || month_abs.join(n).exists());
    let folder = month.join(&name);
    std::fs::create_dir_all(&month_abs).map_err(|e| format!("could not make the folder “{}”: {e}", rel_to_string(&month)))?;
    let temp = loop {
        let t = month_abs.join(format!(".import-docx-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
        match std::fs::create_dir(&t) {
            Ok(()) => break TempFolder(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("could not make a folder in the vault: {e}")),
        }
    };
    let dir = temp.0.clone().expect("held");
    let files = dir.join("files");
    std::fs::create_dir(&files).map_err(|e| format!("could not make a folder in the vault: {e}"))?;
    let filter = dir.join("filter.lua");
    crate::fsutil::create_new(&filter, FILTER.as_bytes()).map_err(|e| format!("could not write the filter: {e}"))?;
    let out = dir.join("note.md");
    drop(folder_step);
    // pandoc runs on a copy with the colours tagged (IMP-041), and on the original when there is none or it fails
    let analysis = tracing::info_span!("flow.import_docx.analyse", peer = "source").in_scope(|| docx_look::analyse(c.docx, &dir.join("look")));
    if let Some(e) = &analysis.error {
        return Err(format!("“{file}” cannot be imported: {e}"));
    }
    let absolute = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut inputs = vec![absolute(c.docx)];
    if let Some(t) = &analysis.tagged {
        inputs.insert(0, absolute(t));
    }
    let mut last = None;
    for input in &inputs {
        let setup = |cmd: &mut std::process::Command| {
            cmd.args(args(input, &filter, &rel_to_string(&folder), &out)).current_dir(&files);
        };
        let go = |cmd| proc::run(cmd, c.timeout, Some(c.cancel), |_| {});
        let run = tracing::info_span!("flow.import_docx.pandoc", peer = "pandoc").in_scope(|| crate::pandoc_limit::run(c.pandoc, setup, go)).map_err(|e| format!("pandoc could not be started: {e}"))?;
        // the memory limit (IMP-062): the same answer for the tagged copy and the original, which would only fill the memory again
        if crate::pandoc_limit::overflowed(&run) {
            return Err(crate::pandoc_limit::overflow_message(Some(&file)));
        }
        match run.ending {
            Ending::Exited(Some(0)) => {
                last = None;
                break;
            }
            Ending::TimedOut => return Err(format!("pandoc needed more than {} seconds for “{file}” and was stopped", c.timeout.as_secs_f64())),
            Ending::Cancelled => return Err("stopped".into()),
            Ending::Exited(_) => last = Some(run.tail.iter().find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).unwrap_or_else(|| "it stopped without a message".into())),
        }
    }
    if let Some(line) = last {
        return Err(format!("pandoc could not read “{file}”: {line}"));
    }
    let text = crate::fsutil::read_to_string(&out).map_err(|e| format!("pandoc made no text for “{file}”: {e}"))?;
    let text = docx_look::colours_to_html(&docx_look::breaks_to_html(&docx_look::aligns_to_html(&text)));
    let kept = format!("{stem}.docx");
    let kept = crate::fsutil::portable_name(&crate::markdown::link_safe_name(&kept), MAX_NAME);
    let kept = if kept.is_empty() || kept.starts_with('.') { "document.docx".to_string() } else { kept };
    if c.keep_original {
        tracing::info_span!("flow.import_docx.keep_original", peer = "vault").in_scope(|| crate::fsutil::copy_new(c.docx, &files.join(&kept))).map_err(|e| format!("could not keep “{file}”: {e}"))?;
    }
    let any = std::fs::read_dir(&files).map(|mut d| d.next().is_some()).unwrap_or(false);
    reserved.insert(name);
    let folder_used = any.then_some(folder);
    Ok(Prepared { text, look: analysis.look.props(), original: folder_used.as_ref().filter(|_| c.keep_original).map(|f| f.join(&kept)), folder: folder_used, temp: Mutex::new(Some(temp.release())) })
}

/// The text of the note: the properties (the original, when kept, then the look of the file,
/// IMP-038, IMP-040, then the tags), the headings lifted when asked (IMP-034, only when the
/// document has no `#` heading) and pandoc's Markdown.
pub fn note_text(p: &Prepared, lift: bool, tags: &[String]) -> String {
    let body = crate::markdown::lift_headings(&p.text).filter(|_| lift).unwrap_or_else(|| p.text.clone());
    let source = p.original.as_ref().map(|o| ("source", yaml(&format!("[[{}]]", rel_to_string(o)))));
    let mut props: Vec<(&str, String)> = source.into_iter().collect();
    props.extend(p.look.iter().map(|(k, v)| (*k, v.clone())));
    if props.is_empty() && tags.is_empty() {
        return body;
    }
    format!("{}\n{body}", front(&props, tags))
}

/// Make the note of a Word item (IMP-035): the files of its folder are put in place, then the note
/// is written; when that fails the folder is taken out again. Returns the note's path.
pub fn import_item(vault: &mut Vault, it: &Item, o: &Options) -> Result<PathBuf, String> {
    let Source::Docx { name, path, pandoc: Some(pandoc), lift, ready } = &it.source else { unreachable!("only Word items with pandoc come here") };
    let _flow = tracing::info_span!("flow.import_docx", peer = "import").entered();
    let prepared = match ready {
        Some(r) => r.clone()?,
        None => {
            // not converted ahead of time (in the dialog's job): do it here
            let cancel = AtomicBool::new(false);
            let c = Convert { pandoc, docx: path, root: &vault.root, date: super::today_ymd(), keep_original: o.keep_docx, timeout: TIMEOUT, cancel: &cancel };
            Arc::new(prepare(&c, &mut HashSet::new())?)
        }
    };
    let title = crate::fsutil::portable_name(&super::safe_title(&it.title), MAX_NAME - 3);
    let title = if title.is_empty() { "Imported".to_string() } else { title };
    let text = note_text(&prepared, *lift, &o.tags);
    tracing::info_span!("flow.import_docx.commit", peer = "vault").in_scope(|| prepared.commit(&vault.root))?;
    let rel = vault.new_note_path_max(&o.folder, &title, crate::fsutil::MADE_NAME_MAX);
    match tracing::info_span!("flow.import_docx.write_note", peer = "vault").in_scope(|| vault.create_file(&rel, &text)) {
        Ok(r) => Ok(r),
        Err(e) => {
            if let Some(f) = &prepared.folder {
                let _ = std::fs::remove_dir_all(vault.root.join(f));
            }
            Err(format!("could not write the note for “{name}”: {e}"))
        }
    }
}
