//! Importing a template set into the vault (TEX-006): from the built-in preset (the TU Darmstadt
//! templates tuda-ci at a pinned commit), from the newest version on CTAN, from a GitHub
//! repository, or from a zip, a `.tar.gz` or a folder of this computer. The set is built whole in
//! a temporary folder (archive read, class files made with docstrip when the set ships only
//! `.dtx`/`.ins`, files and manifests laid out as `sets.rs` says), copied next to its place in the
//! vault, and moved into place in one step, so a vault never holds half a set. A set that is there
//! already is updated: its folder goes to the vault's trash and the person's `settings.toml` and
//! `logo/` are carried over. The job reports progress and can be cancelled; a cancelled or failed
//! import leaves the vault as it was.

use super::archive;
use super::compile::WorkDir;
use super::ctan;
use super::engine::Engine;
use super::install::{download, Handle, Stage};
use super::proc::{self, Ending};
use super::sets::{self, SetMeta, SetSettings, Source};
use crate::gitsync::TreeLock;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The built-in preset: the tuda-ci tests were run against this commit of
/// <https://github.com/tudace/tuda_latex_templates> (version 4.06; there is no tag), and the
/// archive GitHub makes of it has this checksum (`tests/tex-env/tex-env.sh` pins the same).
pub const PRESET_REPO: &str = "tudace/tuda_latex_templates";
pub const PRESET_COMMIT: &str = "e933be93b07008edc9ce77e33b877d15489a476f";
pub const PRESET_SHA256: &str = "477060aeb6e459d661baac1942e83a2bd45e7dd9cdfdff9c3c24f72006b5c6d3";

/// The newest tuda-ci on CTAN: the redirector picks a mirror, the archive has the same layout.
pub const CTAN_URL: &str = "https://mirrors.ctan.org/macros/latex/contrib/tuda-ci.zip";

/// How long docstrip may run for one `.ins` file.
const DOCSTRIP_TIMEOUT: Duration = Duration::from_secs(120);

/// How long the tree lock of the vault is waited for when the set is moved into place.
pub const TREE_WAIT: Duration = Duration::from_secs(30);

/// A set Shale knows: its manifests are shipped with the program (`assets/tex/sets/`) and put into
/// the vault when the files of the set are recognised.
pub struct Builtin {
    pub id: &'static str,
    pub name: &'static str,
    /// A file of `tex/` that only this set has (made by docstrip).
    pub signature: &'static str,
    /// The class that says the version: `\ProvidesExplClass{tudapub}{date}{version}`.
    pub version_class: &'static str,
    pub logo_file: &'static str,
    pub accent: &'static str,
    /// `(id, text of kinds/<id>.toml)`.
    pub kinds: &'static [(&'static str, &'static str)],
    pub readme: &'static str,
}

macro_rules! kind {
    ($id:literal) => {
        ($id, include_str!(concat!("../../../assets/tex/sets/tuda/kinds/", $id, ".toml")))
    };
}

/// The TU Darmstadt templates (tuda-ci): 11 document kinds.
pub const TUDA: Builtin = Builtin {
    id: "tuda",
    name: "TU Darmstadt (tuda-ci)",
    signature: "tudapub.cls",
    version_class: "tudapub",
    logo_file: "tuda_logo",
    accent: "9c",
    kinds: &[kind!("pub"), kind!("report"), kind!("exercise"), kind!("thesis"), kind!("phd"), kind!("beamer"), kind!("letter"), kind!("poster"), kind!("sciposter"), kind!("leaflet"), kind!("announcement")],
    readme: include_str!("../../../assets/tex/sets/tuda/README.md"),
};

/// The note of a set that has no manifests of Shale's.
const UNKNOWN_README: &str = include_str!("../../../assets/tex/sets/unknown-README.md");

/// What to import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// The built-in preset: tuda-ci at the pinned commit, checked by its checksum.
    Preset,
    /// The newest tuda-ci on CTAN (no checksum).
    Ctan,
    /// A GitHub repository (`https://github.com/owner/repo`) at a commit, tag or branch (empty: the default branch).
    Github { url: String, reference: String },
    /// A `.zip`, a `.tar.gz` or a folder of this computer.
    Local(PathBuf),
}

/// An import job.
pub struct Request {
    pub vault: PathBuf,
    pub what: What,
    /// The TeX that runs docstrip when the set needs it.
    pub engine: Option<Engine>,
    /// The lock on the vault's files: the set is moved into place while it is held.
    pub tree: TreeLock,
    /// How long to wait for the lock ([`TREE_WAIT`]).
    pub tree_wait: Duration,
}

/// What an import made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    /// The folder name below `templates/tex`.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    /// How many document kinds the set has: none for a set Shale has no manifests for.
    pub kinds: usize,
    /// The set was there already and has been updated.
    pub replaced: bool,
    /// The set needs a logo and has none yet.
    pub logo_needed: bool,
}

impl Imported {
    /// What the person is told.
    pub fn notice(&self) -> String {
        let what = format!("{}{}", self.name, self.version.as_ref().map(|v| format!(" {v}")).unwrap_or_default());
        let verb = if self.replaced { "updated (your settings and logo were kept; the old files are in the vault's .trash folder)" } else { "imported" };
        if self.kinds == 0 {
            format!("{what} was {verb}, but Shale has no document kinds for it: the kinds must be added by hand in its kinds folder (see the README in the set's folder).")
        } else if self.logo_needed {
            format!("{what} was {verb}: {} document kinds. The logo is missing: choose it, or exports use a placeholder and are not CI-conformant.", self.kinds)
        } else {
            format!("{what} was {verb}: {} document kinds.", self.kinds)
        }
    }
}

// ---------------------------------------------------------------- sources

/// `owner/repo` and the reference from a GitHub address and the reference field.
pub fn parse_github(url: &str, reference: &str) -> Result<(String, String), String> {
    let bad = || format!("{url:?} is not a GitHub repository address (https://github.com/owner/repository)");
    let rest = url.trim().strip_prefix("https://github.com/").ok_or_else(bad)?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let (owner, name) = (parts.next().ok_or_else(bad)?, parts.next().ok_or_else(bad)?);
    let name = name.strip_suffix(".git").unwrap_or(name);
    let part_ok = |p: &str| !p.is_empty() && !p.starts_with(['.', '-']) && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if !part_ok(owner) || !part_ok(name) {
        return Err(bad());
    }
    let from_url = match (parts.next(), parts.next()) {
        (Some("tree"), Some(first)) => Some(std::iter::once(first).chain(parts).collect::<Vec<_>>().join("/")),
        (None, _) => None,
        _ => return Err(bad()),
    };
    let reference = match reference.trim() {
        "" => from_url.unwrap_or_else(|| "HEAD".into()),
        r => r.to_string(),
    };
    let ref_ok = !reference.starts_with(['-', '/', '.']) && !reference.split('/').any(|p| p.is_empty() || p == "..") && reference.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c));
    if !ref_ok {
        return Err(format!("{reference:?} is not a branch, tag or commit name"));
    }
    Ok((format!("{owner}/{name}"), reference))
}

/// The address of GitHub's archive of a reference.
pub fn github_archive_url(repo: &str, reference: &str) -> String {
    format!("https://github.com/{repo}/archive/{reference}.tar.gz")
}

/// The archive must be the one that was tested: refused when its SHA-256 is another.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
    let got = archive::sha256_hex(bytes);
    if got == expected {
        Ok(())
    } else {
        Err(format!("the downloaded archive has the checksum {got}, but {expected} was expected: it was not imported"))
    }
}

/// The archive or folder in `work/src`, and where it came from.
fn fetch(req: &Request, handle: &Handle, work: &WorkDir) -> Result<(PathBuf, Source, Option<String>), String> {
    let src = work.path().join("src");
    std::fs::create_dir(&src).map_err(|e| e.to_string())?;
    let archive_file = work.path().join("archive");
    // (url, source, expected checksum, name for the set)
    let download_and_read = |url: &str, expected: Option<&str>| -> Result<(), String> {
        handle.stage(Stage::Fetch);
        download(handle, url, &archive_file)?;
        let bytes = crate::fsutil::read(&archive_file).map_err(|e| e.to_string())?;
        if let Some(want) = expected {
            verify_sha256(&bytes, want)?;
        }
        handle.stage(Stage::Read);
        archive::extract(&bytes, &src)
    };
    match &req.what {
        What::Preset => {
            download_and_read(&github_archive_url(PRESET_REPO, PRESET_COMMIT), Some(PRESET_SHA256))?;
            Ok((archive::single_root(&src), Source::Github { repo: PRESET_REPO.into(), reference: PRESET_COMMIT.into(), sha256: Some(PRESET_SHA256.into()) }, None))
        }
        What::Ctan => {
            handle.stage(Stage::Fetch);
            ctan::fetch(handle, CTAN_URL, &archive_file, &src)?;
            Ok((archive::single_root(&src), Source::Ctan { url: CTAN_URL.into() }, None))
        }
        What::Github { url, reference } => {
            let (repo, reference) = parse_github(url, reference)?;
            download_and_read(&github_archive_url(&repo, &reference), None)?;
            let name = repo.rsplit('/').next().map(str::to_string);
            Ok((archive::single_root(&src), Source::Github { repo, reference, sha256: None }, name))
        }
        What::Local(path) => {
            handle.stage(Stage::Read);
            let meta = std::fs::metadata(path).map_err(|e| format!("{} cannot be read ({e})", path.display()))?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if meta.is_dir() {
                archive::copy_folder(path, &src)?;
                return Ok((src, Source::Local { path: path.to_string_lossy().into_owned() }, name));
            }
            if meta.len() > archive::MAX_TOTAL {
                return Err(format!("{} is larger than {} MB: refused", path.display(), archive::MAX_TOTAL >> 20));
            }
            archive::extract(&crate::fsutil::read(path).map_err(|e| e.to_string())?, &src)?;
            Ok((archive::single_root(&src), Source::Local { path: path.to_string_lossy().into_owned() }, name))
        }
    }
}

// ---------------------------------------------------------------- building the set

/// Folders that hold examples, documentation and tests, not class files.
fn skipped_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "examples" | "example" | "doc" | "docs" | "test" | "tests")
}

/// The files of `dir` and below, as paths relative to `base`, in order; skipped folders left out.
fn files_below(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(t) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if t.is_dir() && !skipped_dir(&name) {
            files_below(base, &e.path(), out);
        } else if t.is_file() {
            out.extend(e.path().strip_prefix(base).map(Path::to_path_buf));
        }
    }
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension().is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A file name docstrip may be started on (it goes on a command line).
fn valid_ins_name(name: &str) -> bool {
    name.ends_with(".ins") && !name.starts_with(['-', '.']) && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}

/// Run docstrip (LuaLaTeX on an `.ins` file) in the folder of the file.
fn docstrip(engine: &Engine, handle: &Handle, dir: &Path, ins: &str) -> Result<(), String> {
    let mut cmd = engine.command("lualatex");
    cmd.current_dir(dir).env("openout_any", "p").args(["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", ins]);
    let run = proc::run(cmd, DOCSTRIP_TIMEOUT, Some(&handle.cancel), |_| {}).map_err(|e| format!("docstrip could not be started ({e}); check Settings → LaTeX"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("docstrip on {ins} took more than {} seconds and was stopped", DOCSTRIP_TIMEOUT.as_secs())),
        Ending::Exited(Some(0)) => Ok(()),
        Ending::Exited(_) => {
            let from = run.tail.len().saturating_sub(8);
            Err(format!("docstrip on {ins} failed:\n{}", run.tail[from..].join("\n")))
        }
    }
}

/// The shallowest folder with class or package files (`tex/` in tuda-ci's archive, the top folder
/// of a repository that keeps them there).
fn class_root(src: &Path) -> Option<PathBuf> {
    let mut level = vec![src.to_path_buf()];
    while !level.is_empty() {
        let mut next = vec![];
        for dir in &level {
            let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.file_name());
            if entries.iter().any(|e| e.file_type().is_ok_and(|t| t.is_file()) && has_ext(&e.path(), &["cls", "sty"])) {
                return Some(dir.clone());
            }
            next.extend(entries.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !skipped_dir(&e.file_name().to_string_lossy())).map(|e| e.path()));
        }
        level = next;
    }
    None
}

/// The file types that go into a set's `tex/`: what TeX reads from `TEXINPUTS`.
const TEX_FILES: [&str; 17] = ["cls", "sty", "def", "cfg", "clo", "lco", "ldf", "fd", "tex", "bst", "cbx", "bbx", "lbx", "dbx", "map", "otf", "ttf"];

/// `\ProvidesExplClass{tudapub}{2026-06-08}{4.06}` as "4.06".
fn provided_version(class_file: &str, class: &str) -> Option<String> {
    let at = class_file.find(&format!("\\ProvidesExplClass{{{class}}}"))?;
    let rest = &class_file[at..];
    let mut groups = rest.split('{').skip(1).map(|g| g.split('}').next().unwrap_or(""));
    let version = groups.nth(2)?.trim();
    (!version.is_empty() && version.len() < 20).then(|| version.to_string())
}

/// Everything of the set in `out`: `tex/`, `kinds/`, `set.toml`, `settings.toml` (new sets) and `README.md`.
fn build(src: &Path, out: &Path, source: Source, derived_name: Option<String>, req: &Request, handle: &Handle, notify: &dyn Fn()) -> Result<Imported, String> {
    let cancelled = || if handle.cancel.load(std::sync::atomic::Ordering::SeqCst) { Err("cancelled".to_string()) } else { Ok(()) };
    // no class files, only .ins and .dtx: make them with docstrip
    let mut all = vec![];
    files_below(src, src, &mut all);
    if !all.iter().any(|p| has_ext(p, &["cls"])) {
        let inses: Vec<&PathBuf> = all.iter().filter(|p| has_ext(p, &["ins"])).collect();
        if !inses.is_empty() {
            let engine = req.engine.as_ref().ok_or("these templates need docstrip to make their class files, and no TeX is usable: install or choose one in this page first")?;
            handle.stage(Stage::Generate);
            notify();
            for ins in inses {
                cancelled()?;
                let name = ins.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if !valid_ins_name(&name) {
                    return Err(format!("{} cannot be run through docstrip (an odd file name)", ins.display()));
                }
                docstrip(engine, handle, &src.join(ins).parent().map(Path::to_path_buf).unwrap_or_else(|| src.to_path_buf()), &name)?;
            }
        }
    }
    cancelled()?;
    let root = class_root(src).ok_or("no class or package files (.cls, .sty) were found in these templates")?;
    let mut files = vec![];
    files_below(&root, &root, &mut files);
    let tex = out.join("tex");
    std::fs::create_dir_all(&tex).map_err(|e| e.to_string())?;
    let mut copied = 0;
    for rel in files.iter().filter(|p| has_ext(p, &TEX_FILES)) {
        let to = tex.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::fsutil::copy_new(&root.join(rel), &to).map_err(|e| format!("{} could not be copied ({e})", rel.display()))?;
        copied += 1;
    }
    if copied == 0 {
        return Err("no class or package files were found in these templates".into());
    }
    let builtin = tex.join(TUDA.signature).is_file().then_some(&TUDA);
    let (id, name, version, logo_file) = match builtin {
        Some(b) => (b.id.to_string(), b.name.to_string(), crate::fsutil::read_to_string(tex.join(b.signature)).ok().and_then(|t| provided_version(&t, b.version_class)), Some(b.logo_file.to_string())),
        None => {
            let derived = derived_name.unwrap_or_else(|| "templates".into());
            (sets::id_from(&derived), sets::strip_archive_ext(&derived).to_string(), None, None)
        }
    };
    let kinds = builtin.map_or(&[][..], |b| b.kinds);
    if !kinds.is_empty() {
        let dir = out.join("kinds");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (kind_id, text) in kinds {
            crate::fsutil::create_new(&dir.join(format!("{kind_id}.toml")), text.as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    let meta = SetMeta { name: name.clone(), version: version.clone(), imported_at: crate::mail::store::today_string(), source, logo_file: logo_file.clone() };
    let meta_text = toml::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    crate::fsutil::create_new(&out.join("set.toml"), meta_text.as_bytes()).map_err(|e| e.to_string())?;
    let settings = SetSettings { accent: builtin.map(|b| b.accent.to_string()).unwrap_or_default(), ..SetSettings::default() };
    sets::save_settings(out, &settings)?;
    let readme = builtin.map_or(UNKNOWN_README, |b| b.readme);
    crate::fsutil::create_new(&out.join("README.md"), readme.as_bytes()).map_err(|e| e.to_string())?;
    Ok(Imported { id, name, version, kinds: kinds.len(), replaced: false, logo_needed: logo_file.is_some() })
}

// ---------------------------------------------------------------- placing it

/// Remove the folders this import made when it ends without placing a set.
fn clean_up(staging: &Path, made: &[PathBuf]) {
    let _ = std::fs::remove_dir_all(staging);
    // the folders made for the set go again when nothing is in them
    for dir in made.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Copy the built set next to its place, carry the person's files over, and move it into place.
fn place(req: &Request, out: &Path, imported: &mut Imported, handle: &Handle) -> Result<(), String> {
    let root = sets::sets_root(&req.vault);
    let mut made = vec![];
    for dir in [req.vault.join("templates"), root.clone()] {
        if !dir.exists() {
            made.push(dir);
        }
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("{} could not be made ({e})", root.display()))?;
    let staging = root.join(format!(".{}.staging", imported.id));
    let target = root.join(&imported.id);
    let result = (|| -> Result<(), String> {
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir(&staging).map_err(|e| format!("{} could not be made ({e})", staging.display()))?;
        archive::copy_folder(out, &staging)?;
        let existing = std::fs::symlink_metadata(&target).ok();
        if existing.as_ref().is_some_and(|m| !m.is_dir()) {
            return Err(format!("{} is there and is not a set folder", target.display()));
        }
        if existing.is_some() {
            imported.replaced = true;
            // the person's choices and logo stay
            let old_settings = target.join("settings.toml");
            if old_settings.is_file() {
                crate::fsutil::write_atomic(&staging.join("settings.toml"), &crate::fsutil::read(&old_settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            }
            let old_logo = target.join("logo");
            if old_logo.is_dir() {
                let new_logo = staging.join("logo");
                std::fs::create_dir(&new_logo).map_err(|e| e.to_string())?;
                archive::copy_folder(&old_logo, &new_logo)?;
            }
        }
        if handle.cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let _lock = req.tree.hold_for(req.tree_wait).ok_or("the sync is still changing files in the vault: try again in a moment")?;
        let rel = Path::new(sets::SETS_DIR).join(&imported.id);
        let trashed = if existing.is_some() { Some(crate::vault::move_to_trash(&req.vault, &rel).map_err(|e| format!("the old files could not be moved to the trash ({e})"))?) } else { None };
        if let Err(e) = crate::fsutil::rename_noreplace(&staging, &target) {
            // put the old set back
            if let Some(t) = trashed {
                let _ = crate::fsutil::rename_noreplace(&req.vault.join(t), &target);
            }
            return Err(format!("the set could not be moved into place ({e})"));
        }
        Ok(())
    })();
    if result.is_err() {
        clean_up(&staging, &made);
    }
    result
}

/// Import a set into the vault (TEX-006). Runs in a job; `notify` asks for a repaint after progress.
pub fn import(req: &Request, handle: &Handle, notify: &dyn Fn()) -> Result<Imported, String> {
    if !req.vault.is_dir() {
        return Err("there is no vault to import into".into());
    }
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    notify();
    let (src, source, name) = tracing::info_span!("flow.tex_setup.fetch", peer = "source").in_scope(|| fetch(req, handle, &work))?;
    if handle.cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    notify();
    let out = work.path().join("out");
    std::fs::create_dir(&out).map_err(|e| e.to_string())?;
    let mut imported = tracing::info_span!("flow.tex_setup.build", peer = "tex").in_scope(|| build(&src, &out, source, name, req, handle, notify))?;
    handle.stage(Stage::Place);
    notify();
    tracing::info_span!("flow.tex_setup.place", peer = "vault").in_scope(|| place(req, &out, &mut imported, handle))?;
    Ok(imported)
}
