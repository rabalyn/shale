//! File history: snapshot commits of the notes, and the queries and actions that restore them
//! (SYNC-030…SYNC-046, CFG-066, ADR-0028).
//!
//! # Public API (for the history panel #472 and the sync wizard #469)
//!
//! Everything here runs `git` and belongs in a background job (JOB-001), except [`Schedule`],
//! [`HistoryConfig`], [`parse_message`], [`tracked`] and [`skipped`], which are pure.
//!
//! - [`locate`]`(git, vault, history_root) -> Result<Repo, String>`: the repository of a vault: its
//!   own `<vault>/.git`, or a separate git directory `<history_root>/<vault id>` (the vault ID is
//!   the file [`ID_FILE`], `.shale/history-id`). Nothing is written into the vault but that file.
//! - [`ensure`]`(&Repo) -> Result<bool, String>`: make a local-only repository (true: it was made).
//! - [`snapshot`]`(&Repo, &Options, &Request, &TreeLock, wait) -> Result<Outcome, String>`: commit the
//!   saved text of some paths.
//! - [`versions`]`(&Repo, &Path, limit) -> Result<Vec<Version>, String>`: the versions of a note,
//!   newest first, following renames.
//! - [`content_at`]`(&Repo, commit, &Path) -> Result<Vec<u8>, String>`: the file at a version.
//! - [`deleted`]`(&Repo, &Path /* vault */, limit) -> Result<Vec<Deleted>, String>` and
//!   [`deleted_content`]`(&Repo, &Deleted)`: the deleted notes and their last text.
//! - [`restored_name`]`(path, taken) -> PathBuf`: a free name for a restored note.
//! - The size of a local-only history is a line of Data usage (USE-025, `data_usage::scan::outside`).
//! - [`parse_message`]`(&str) -> Meta`: the trailers of a commit message ([`Reason`], device, paths).
//!
//! # Layout
//!
//! A vault without `.git` has the git directory `<data folder>/shale/history/<vault id>`: a normal
//! git directory (`HEAD` on `main`, `objects/`, `refs/`, `config` with `core.bare=false` and
//! `core.autocrlf=false`) plus a file `worktree` with the vault folder it belongs to. Git runs with
//! `--git-dir=<it> --work-tree=<vault>`.

use super::{ensure_idle, Git, Options, Out, TreeLock};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An argument list from anything that is a string or a path.
macro_rules! osv {
    ($($a:expr),* $(,)?) => { [$(OsString::from($a)),*] };
}

/// Where the histories of the vaults without `.git` live: `<data folder>/shale/history`.
pub fn default_root() -> PathBuf {
    crate::userdirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("history")
}

/// The vault's ID, relative to the vault: 128 random bits in hex, one line. Synced with the vault
/// (ADR-0005), so the same vault has the same ID on every device.
pub const ID_FILE: &str = ".shale/history-id";

/// The file in a git directory that names the folder it belongs to.
pub(crate) const WORKTREE_FILE: &str = "worktree";

/// Names that no snapshot takes: Shale's temporary files and what cloud clients and network
/// shares make next to the notes (`*` is any text; compared without letter case).
const SKIPPED: &[&str] = &[".shale-tmp-*", ".syncthing.*.tmp", ".stfolder", ".stversions", ".stignore", ".~lock.*#", "~$*", "*.tmp", "*.part", ".nfs*", ".fuse_hidden*", "*.icloud", "desktop.ini", ".ds_store", "thumbs.db"];

/// The history settings (CFG-066).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct HistoryConfig {
    /// Snapshots are made (SYNC-032).
    pub enabled: bool,
    /// At most one snapshot per note in this many minutes while typing (SYNC-033).
    pub coalesce_minutes: u32,
    /// A snapshot when a note had no change for this many seconds (SYNC-033).
    pub idle_seconds: u32,
    /// The history column beside a note is shown when there is room (HIST-003).
    pub show_panel: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        HistoryConfig { enabled: true, coalesce_minutes: 10, idle_seconds: 60, show_panel: true }
    }
}

impl HistoryConfig {
    /// The window in seconds, brought into 1 minute…1 day.
    pub fn coalesce_secs(&self) -> u64 {
        u64::from(self.coalesce_minutes.clamp(1, 1440)) * 60
    }

    /// The idle time in seconds, brought into 5 s…1 hour.
    pub fn idle_secs(&self) -> u64 {
        u64::from(self.idle_seconds.clamp(5, 3600))
    }
}

/// Why a snapshot was made (`Shale-Reason:`, SYNC-036).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Save,
    Idle,
    Close,
    Initial,
    Sync,
    Restore,
    Merge,
    Rename,
    Delete,
    /// an import wrote notes (IMP-072)
    Import,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Save => "save",
            Reason::Idle => "idle",
            Reason::Close => "close",
            Reason::Initial => "initial",
            Reason::Sync => "sync",
            Reason::Restore => "restore",
            Reason::Merge => "merge",
            Reason::Rename => "rename",
            Reason::Delete => "delete",
            Reason::Import => "import",
        }
    }

    pub fn parse(s: &str) -> Option<Reason> {
        [Reason::Save, Reason::Idle, Reason::Close, Reason::Initial, Reason::Sync, Reason::Restore, Reason::Merge, Reason::Rename, Reason::Delete, Reason::Import].into_iter().find(|r| r.as_str() == s.trim())
    }
}

/// What a commit message says (SYNC-036); every field but the subject is empty for a commit that
/// another program made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub subject: String,
    pub reason: Option<Reason>,
    pub device: Option<String>,
    pub paths: Vec<String>,
    /// The commit a restore went back to.
    pub restored: Option<String>,
}

/// Read the subject and the `Shale-…:` trailers of a commit message. A message without trailers
/// (made by another tool, or by an older Shale) gives its subject only.
pub fn parse_message(message: &str) -> Meta {
    let mut meta = Meta::default();
    let mut lines = message.lines();
    meta.subject = lines.next().unwrap_or("").trim().to_string();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "Shale-Reason" => meta.reason = Reason::parse(value),
            "Shale-Device" if !value.is_empty() => meta.device = Some(value.to_string()),
            "Shale-Path" if !value.is_empty() => meta.paths.push(value.to_string()),
            "Shale-Restored" if !value.is_empty() => meta.restored = Some(value.to_string()),
            _ => {}
        }
    }
    meta
}

/// The trailers of a message (the paragraph after the subject).
pub(super) fn trailers(reason: Reason, device: &str, paths: &[String], restored: Option<&str>) -> String {
    let clean = |s: &str| s.chars().filter(|c| !c.is_control()).collect::<String>();
    let mut out = format!("Shale-Reason: {}\nShale-Device: {}", reason.as_str(), clean(device));
    for p in paths {
        if !p.chars().any(char::is_control) {
            out.push_str(&format!("\nShale-Path: {p}"));
        }
    }
    if let Some(r) = restored {
        out.push_str(&format!("\nShale-Restored: {}", clean(r)));
    }
    out
}

/// `*` matches any text; the rest matches itself, letter case ignored.
fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some((c, rest)) => t.first().is_some_and(|x| x == c) && go(rest, &t[1..]),
        }
    }
    let (p, t): (Vec<char>, Vec<char>) = (pattern.to_lowercase().chars().collect(), text.to_lowercase().chars().collect());
    go(&p, &t)
}

/// A path no snapshot takes: a skipped name (SYNC-034) anywhere in it, or a device-local file of
/// `.shale/` (`state.json`, `undo/`).
pub fn skipped(rel: &Path) -> bool {
    let names: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if names.first().is_some_and(|n| n == ".shale") && names.get(1).is_some_and(|n| n == "undo" || n == "state.json") {
        return true;
    }
    names.iter().any(|n| SKIPPED.iter().any(|p| glob_match(p, n)))
}

/// A path a snapshot takes: a note or a drawing (VAULT-002) outside hidden folders and not skipped.
pub fn tracked(rel: &Path) -> bool {
    crate::vault::kind_of(rel) != crate::vault::FileKind::Other && !rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) && !skipped(rel)
}

/// The repository of one vault: the git to run, the vault, and the git directory when it is not
/// the vault's own `.git`.
#[derive(Clone, Debug)]
pub struct Repo {
    pub git: Git,
    pub vault: PathBuf,
    /// None: the vault's own `.git` (syncing or an existing repository).
    pub git_dir: Option<PathBuf>,
}

impl Repo {
    /// Run `git -C <vault> [--git-dir=… --work-tree=…] args…`.
    fn run(&self, args: &[OsString]) -> Result<Out, String> {
        let mut all: Vec<OsString> = vec![];
        if let Some(gd) = &self.git_dir {
            let mut a = OsString::from("--git-dir=");
            a.push(gd);
            let mut w = OsString::from("--work-tree=");
            w.push(&self.vault);
            all.extend([a, w]);
        }
        all.extend(args.iter().cloned());
        let refs: Vec<&OsStr> = all.iter().map(OsString::as_os_str).collect();
        self.git.run(&self.vault, &refs)
    }

    /// Tests: run git in the repository and return its stdout.
    #[cfg(test)]
    pub(crate) fn run_for_test(&self, args: &[&str]) -> String {
        let a: Vec<OsString> = args.iter().map(OsString::from).collect();
        String::from_utf8_lossy(&self.run(&a).unwrap().stdout).into_owned()
    }

    /// Run and require success; the error is git's own message.
    fn ok(&self, args: &[OsString]) -> Result<Out, String> {
        let o = self.run(args)?;
        if o.ok {
            Ok(o)
        } else {
            let what = args.iter().take(3).map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
            Err(if o.stderr.is_empty() { format!("git {what} failed") } else { format!("git {what}: {}", o.stderr) })
        }
    }

    /// `-c user.name=… -c user.email=…` when git has no identity.
    fn identity(&self) -> Result<Vec<OsString>, String> {
        let mut out = vec![];
        if self.run(&osv!["config", "user.name"])?.text().is_empty() {
            out.extend(osv!["-c", "user.name=Shale"]);
        }
        if self.run(&osv!["config", "user.email"])?.text().is_empty() {
            out.extend(osv!["-c", "user.email=shale@localhost"]);
        }
        Ok(out)
    }
}

/// An argument list from anything that is a string or a path.
macro_rules! osv {
    ($($a:expr),* $(,)?) => { [$(OsString::from($a)),*] };
}

/// The ID of the vault, if it has one.
pub fn vault_id(vault: &Path) -> Option<String> {
    let text = crate::fsutil::read_to_string(vault.join(ID_FILE)).ok()?;
    let id = text.trim();
    (id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_ascii_lowercase())
}

/// The ID of the vault; a vault without one gets one (written to [`ID_FILE`]).
pub fn ensure_vault_id(vault: &Path) -> Result<String, String> {
    if let Some(i) = vault_id(vault) {
        return Ok(i);
    }
    let i = new_id();
    write_id(vault, &i)?;
    Ok(i)
}

fn new_id() -> String {
    crate::mail::compose::random_hex(16)
}

fn same_folder(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

/// The repository of `vault` (SYNC-030, SYNC-031). A vault with `.git` uses it. Otherwise the
/// repository is `<history_root>/<vault id>`; a vault without an ID gets one (written to
/// [`ID_FILE`]), and so does a copy of a vault whose original still exists with the same ID.
/// The repository itself is made by [`ensure`].
pub fn locate(git: &Git, vault: &Path, history_root: &Path) -> Result<Repo, String> {
    if super::is_repo(vault) {
        return Ok(Repo { git: git.clone(), vault: vault.to_path_buf(), git_dir: None });
    }
    Ok(Repo { git: git.clone(), vault: vault.to_path_buf(), git_dir: Some(local_git_dir(vault, history_root)?) })
}

/// The git directory a vault without `.git` has: `<history_root>/<vault id>` (SYNC-031). A vault
/// without an ID gets one (written to [`ID_FILE`]), and so does a copy of a vault whose original
/// still exists with the same ID. The wizard's move of a repository out of the vault (SYNC-063)
/// uses it too.
pub fn local_git_dir(vault: &Path, history_root: &Path) -> Result<PathBuf, String> {
    let mut id = match vault_id(vault) {
        Some(i) => i,
        None => {
            let i = new_id();
            write_id(vault, &i)?;
            i
        }
    };
    // a copy has the ID of a folder that is still there, and gets a new one (a new ID is not taken)
    let owner = |id: &str| crate::fsutil::read_to_string(history_root.join(id).join(WORKTREE_FILE)).ok().map(|s| PathBuf::from(s.trim()));
    if owner(&id).is_some_and(|o| !same_folder(&o, vault) && o.is_dir() && vault_id(&o).as_deref() == Some(id.as_str())) {
        id = new_id();
        write_id(vault, &id)?;
    }
    Ok(history_root.join(id))
}

fn write_id(vault: &Path, id: &str) -> Result<(), String> {
    let p = vault.join(ID_FILE);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("could not make {}: {e}", d.display()))?;
    }
    crate::fsutil::write_atomic(&p, format!("{id}\n").as_bytes()).map_err(|e| format!("could not write {}: {e}", p.display()))
}

/// Make the repository of a local-only history when it is not there, and say where it works
/// (SYNC-044: the caller then makes the first snapshot). True: it was made now.
pub fn ensure(repo: &Repo) -> Result<bool, String> {
    let Some(gd) = &repo.git_dir else { return Ok(false) };
    let made = !gd.join("HEAD").exists();
    if made {
        std::fs::create_dir_all(gd).map_err(|e| format!("could not make {}: {e}", gd.display()))?;
        repo.ok(&osv!["init", "--quiet"])?;
        // a work tree is given with every command: the exact bytes of the notes, not git's line endings
        repo.ok(&osv!["config", "core.bare", "false"])?;
        repo.ok(&osv!["config", "core.autocrlf", "false"])?;
        repo.ok(&osv!["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    let want = repo.vault.canonicalize().unwrap_or_else(|_| repo.vault.clone());
    if crate::fsutil::read_to_string(gd.join(WORKTREE_FILE)).ok().map(|s| PathBuf::from(s.trim())).as_deref() != Some(want.as_path()) {
        crate::fsutil::write_atomic(&gd.join(WORKTREE_FILE), want.to_string_lossy().as_bytes()).map_err(|e| format!("could not write {}: {e}", gd.join(WORKTREE_FILE).display()))?;
    }
    Ok(made)
}

/// What to snapshot, and why.
#[derive(Clone, Debug)]
pub struct Request {
    /// Vault-relative paths (a folder stands for the notes and drawings in it); empty with
    /// [`Reason::Initial`]: every note and drawing.
    pub paths: Vec<PathBuf>,
    pub reason: Reason,
    /// The commit a restore went back to.
    pub restored: Option<String>,
}

impl Request {
    pub fn new(reason: Reason, paths: Vec<PathBuf>) -> Request {
        Request { paths, reason, restored: None }
    }
}

/// What a snapshot did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A commit with this id was made.
    Committed(String),
    /// The paths equal what is committed: no commit.
    Unchanged,
    /// Not now (the tree lock, `index.lock`, a merge or a rebase in progress, a detached HEAD):
    /// ask again later.
    Busy(String),
}

fn pathspec(p: &Path) -> OsString {
    let mut s = OsString::from(":(literal)");
    s.push(crate::vault::rel_to_string(p));
    s
}

/// `*?[\` of a folder name made literal for a glob pathspec.
fn glob_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "*?[\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The pathspecs of a request: literal files, and for a folder the glob specs of the notes and
/// drawings below it (one `git add` per group, since a pathspec that matches nothing fails the whole add).
struct Plan {
    literal: Vec<OsString>,
    globs: Vec<Vec<OsString>>,
}

fn excludes() -> Vec<OsString> {
    let mut out = vec![OsString::from(":(exclude,glob)**/.*"), OsString::from(":(exclude,glob)**/.*/**")];
    for p in SKIPPED {
        out.push(format!(":(exclude,glob,icase)**/{p}").into());
        out.push(format!(":(exclude,glob,icase)**/{p}/**").into());
    }
    out
}

/// One `git add` per kind: a pathspec that matches nothing makes git add nothing at all.
fn kind_globs(dir: &str) -> Vec<Vec<OsString>> {
    let base = if dir.is_empty() { String::new() } else { format!("{}/", glob_escape(dir)) };
    ["*.md", "*.excalidraw", "*.canvas"]
        .iter()
        .map(|g| {
            let mut v: Vec<OsString> = vec![OsString::from(format!(":(icase,glob){base}**/{g}"))];
            v.extend(excludes());
            v
        })
        .collect()
}

impl Plan {
    fn all(&self) -> Vec<OsString> {
        let mut out: Vec<OsString> = vec![];
        for s in self.literal.iter().chain(self.globs.iter().flatten()) {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
        out
    }
}

fn plan(repo: &Repo, req: &Request) -> Result<Plan, String> {
    let mut plan = Plan { literal: vec![], globs: vec![] };
    if req.reason == Reason::Initial {
        plan.globs.extend(kind_globs(""));
        return Ok(plan);
    }
    let mut seen = HashSet::new();
    for p in &req.paths {
        if !seen.insert(p.clone()) || p.as_os_str().is_empty() || p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            continue;
        }
        let abs = repo.vault.join(p);
        if abs.is_dir() {
            if !skipped(p) && !p.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
                plan.globs.extend(kind_globs(&crate::vault::rel_to_string(p)));
            }
        } else if abs.symlink_metadata().is_ok() {
            if tracked(p) {
                plan.literal.push(pathspec(p));
            }
        } else if !skipped(p) && !p.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
            // gone: only what the repository knows can change (a deletion or the old side of a move)
            let known = repo.run(&osv!["--literal-pathspecs", "ls-files", "-z", "--", crate::vault::rel_to_string(p)])?;
            let names: Vec<&[u8]> = known.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()).collect();
            let is_file = names.len() == 1 && names[0] == crate::vault::rel_to_string(p).as_bytes();
            if known.ok && !names.is_empty() && (!is_file || tracked(p)) {
                plan.literal.push(pathspec(p));
            }
        }
    }
    Ok(plan)
}

fn subject(req: &Request) -> String {
    let names: Vec<String> = req.paths.iter().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
    match (req.reason, names.as_slice()) {
        (Reason::Initial, _) => "Start of the history".to_string(),
        (Reason::Rename, [a, b]) => format!("Rename {a} to {b}"),
        // the moved path first, its new place second, then the notes whose links were updated
        (Reason::Rename, [a, b, more @ ..]) => format!("Rename {a} to {b}, {} link update(s)", more.len()),
        (Reason::Rename, n) => format!("Rename {} file(s)", n.len()),
        (Reason::Delete, [a]) => format!("Delete {a}"),
        (Reason::Delete, n) => format!("Delete {} file(s)", n.len()),
        (Reason::Restore, [a]) => format!("Restore {a}"),
        (Reason::Restore, n) => format!("Restore {} file(s)", n.len()),
        (_, [a]) => format!("Snapshot of {a}"),
        (_, n) => format!("Snapshot of {} files", n.len()),
    }
}

fn busy(e: String) -> Result<Outcome, String> {
    if e.contains("index.lock") || e.contains("Unable to create") {
        Ok(Outcome::Busy(e))
    } else {
        Err(e)
    }
}

/// Commit the saved text of the paths of `req` (SYNC-033…SYNC-036): `git add -A` and a commit of
/// those paths only, so other staged changes stay as they are; no fetch, no push. Takes the tree
/// lock for the time it runs and gives up after `wait` (then [`Outcome::Busy`]). A local-only
/// repository is made first ([`ensure`]), with one [`Reason::Initial`] snapshot of the vault.
pub fn snapshot(repo: &Repo, opts: &Options, req: &Request, tree: &TreeLock, wait: Duration) -> Result<Outcome, String> {
    let Some(_tree) = tree.hold_for(wait) else { return Ok(Outcome::Busy("the vault's files are in use".into())) };
    snapshot_held(repo, opts, req)
}

/// Like [`snapshot`], for a caller that holds the tree lock already (the window while it moves a
/// note to the trash).
pub fn snapshot_held(repo: &Repo, opts: &Options, req: &Request) -> Result<Outcome, String> {
    let _flow = tracing::info_span!("flow.history_snapshot", peer = "git").entered();
    let made = ensure(repo)?;
    if repo.git_dir.is_none() {
        if let Err(e) = ensure_idle(&repo.git, &repo.vault) {
            return Ok(Outcome::Busy(e));
        }
    }
    if req.reason == Reason::Initial && !made {
        return Ok(Outcome::Unchanged); // the history has a start already (SYNC-044)
    }
    if made && req.reason != Reason::Initial {
        // the first snapshot of a new history is the vault as it is (SYNC-044), then this one
        let first = Request::new(Reason::Initial, vec![]);
        if let Outcome::Busy(why) = commit(repo, opts, &first)? {
            return Ok(Outcome::Busy(why));
        }
    }
    commit(repo, opts, req)
}

fn commit(repo: &Repo, opts: &Options, req: &Request) -> Result<Outcome, String> {
    let plan = plan(repo, req)?;
    let specs = plan.all();
    if specs.is_empty() {
        return Ok(Outcome::Unchanged);
    }
    let in_vault = repo.git_dir.is_none();
    if !plan.literal.is_empty() {
        let mut a = osv!["add", "-A", "--"].to_vec();
        a.extend(plan.literal.iter().cloned());
        if let Err(e) = repo.ok(&a) {
            return busy(e);
        }
    }
    for g in &plan.globs {
        let mut a = osv!["add", "-A", "--"].to_vec();
        a.extend(g.iter().cloned());
        let o = repo.run(&a)?;
        if !o.ok && !o.stderr.contains("did not match any files") {
            return busy(format!("git add: {}", o.stderr));
        }
    }
    let mut d = osv!["diff", "--cached", "--name-only", "--no-renames", "-z", "--"].to_vec();
    d.extend(specs);
    let staged: Vec<OsString> = repo.ok(&d)?.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| OsString::from(String::from_utf8_lossy(s).into_owned())).collect();
    if staged.is_empty() {
        return Ok(Outcome::Unchanged);
    }
    let paths: Vec<String> = if req.reason == Reason::Initial { vec![] } else { req.paths.iter().map(|p| crate::vault::rel_to_string(p)).collect() };
    let mut msg = subject(req);
    if in_vault {
        msg = format!("{}{msg}{}", opts.commit_prefix, opts.commit_suffix);
    }
    msg.push_str("\n\n");
    msg.push_str(&trailers(req.reason, &opts.host, &paths, req.restored.as_deref()));
    let mut c = repo.identity()?;
    c.extend(osv!["-c", "commit.gpgsign=false", "commit", "--quiet", "--no-verify", "-m", msg]);
    if in_vault {
        // only these paths, never what else is staged in the repository
        c.extend(osv!["--only", "--"]);
        c.extend(staged.iter().map(|n| {
            let mut s = OsString::from(":(literal)");
            s.push(n);
            s
        }));
    }
    if let Err(e) = repo.ok(&c) {
        return busy(e);
    }
    let head = repo.ok(&osv!["rev-parse", "HEAD"])?.text();
    Ok(Outcome::Committed(head))
}

/// One version of a note (SYNC-043).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub commit: String,
    /// Seconds since 1970 (the committer date).
    pub time: i64,
    pub meta: Meta,
    /// The note's path at this version (it differs from today's after a rename).
    pub path: PathBuf,
    /// Lines added and removed; None for a binary file (SYNC-042).
    pub lines: Option<(u32, u32)>,
    pub binary: bool,
}

const RECORD: char = '\u{1e}';
const FIELD: char = '\u{1f}';
const LOG_FORMAT: &str = "--format=%x1e%H%x1f%ct%x1f%B";

/// The header of a log record (commit, time, message) and what follows it.
fn split_records(out: &[u8]) -> Vec<(String, i64, String, Vec<u8>)> {
    let mut records = vec![];
    for chunk in out.split(|b| *b == RECORD as u8).skip(1) {
        let Some(end) = chunk.iter().position(|b| *b == 0) else { continue };
        let head = String::from_utf8_lossy(&chunk[..end]).into_owned();
        let mut f = head.splitn(3, FIELD);
        let (commit, time, message) = (f.next().unwrap_or("").to_string(), f.next().unwrap_or("0").parse().unwrap_or(0), f.next().unwrap_or("").to_string());
        let rest = chunk[end + 1..].strip_prefix(b"\n").unwrap_or(&chunk[end + 1..]).to_vec();
        records.push((commit, time, message, rest));
    }
    records
}

/// The versions of `path`, newest first, at most `limit`, following renames (SYNC-041). A commit
/// that did not change the file (a merge) is not a version.
pub fn versions(repo: &Repo, path: &Path, limit: usize) -> Result<Vec<Version>, String> {
    let _flow = tracing::info_span!("flow.history_versions", peer = "git").entered();
    if repo.run(&osv!["rev-parse", "--verify", "--quiet", "HEAD"])?.stdout.is_empty() {
        return Ok(vec![]); // nothing committed yet
    }
    let mut a = osv!["--literal-pathspecs", "log", "--follow", "-z", "-M", "--numstat", LOG_FORMAT].to_vec();
    a.extend(osv!["-n", limit.to_string(), "--", crate::vault::rel_to_string(path)]);
    let out = repo.ok(&a)?;
    let mut list = vec![];
    for (commit, time, message, rest) in split_records(&out.stdout) {
        let mut tokens = rest.split(|b| *b == 0).filter(|t| !t.is_empty());
        let Some(first) = tokens.next() else { continue };
        let first = String::from_utf8_lossy(first).into_owned();
        let mut parts = first.splitn(3, '\t');
        let (added, removed, name) = (parts.next().unwrap_or("-"), parts.next().unwrap_or("-"), parts.next().unwrap_or(""));
        let at = if name.is_empty() {
            // a rename: the old name, then the new one
            let _old = tokens.next();
            tokens.next().map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default()
        } else {
            name.to_string()
        };
        let lines = added.parse::<u32>().ok().zip(removed.parse::<u32>().ok());
        list.push(Version { commit, time, meta: parse_message(&message), path: PathBuf::from(at), lines, binary: lines.is_none() });
    }
    Ok(list)
}

/// The file `path` as it was at `commit` (SYNC-043).
pub fn content_at(repo: &Repo, commit: &str, path: &Path) -> Result<Vec<u8>, String> {
    if commit.starts_with('-') {
        return Err("not a commit".into());
    }
    Ok(repo.ok(&osv!["cat-file", "blob", format!("{commit}:{}", crate::vault::rel_to_string(path))])?.stdout)
}

/// A deleted note or drawing (SYNC-040).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deleted {
    pub path: PathBuf,
    /// The commit that deleted it (or moved it to the trash).
    pub commit: String,
    pub time: i64,
    pub meta: Meta,
}

/// The notes and drawings that were deleted (or moved to the trash) and are not there now, newest
/// first, at most `limit`, from the last 5000 commits (SYNC-040).
pub fn deleted(repo: &Repo, limit: usize) -> Result<Vec<Deleted>, String> {
    let _flow = tracing::info_span!("flow.history_deleted", peer = "git").entered();
    if repo.run(&osv!["rev-parse", "--verify", "--quiet", "HEAD"])?.stdout.is_empty() {
        return Ok(vec![]);
    }
    let out = repo.ok(&osv!["log", "-z", "-M", "--name-status", "--diff-filter=DR", LOG_FORMAT, "-n", "5000"])?;
    let mut list: Vec<Deleted> = vec![];
    let mut seen = HashSet::new();
    for (commit, time, message, rest) in split_records(&out.stdout) {
        let meta = parse_message(&message);
        let mut tokens = rest.split(|b| *b == 0).filter(|t| !t.is_empty()).map(|t| String::from_utf8_lossy(t).into_owned());
        while let Some(status) = tokens.next() {
            let (gone, to) = match status.chars().next() {
                Some('R') => {
                    let (old, new) = (tokens.next().unwrap_or_default(), tokens.next().unwrap_or_default());
                    (old, Some(new))
                }
                _ => (tokens.next().unwrap_or_default(), None),
            };
            // a move to the trash is a deletion; any other move keeps the note
            if to.as_deref().is_some_and(|n| !n.starts_with(".trash/")) {
                continue;
            }
            let path = PathBuf::from(&gone);
            let hidden = gone.split('/').any(|c| c.starts_with('.'));
            if hidden || !tracked(&path) || repo.vault.join(&path).exists() || !seen.insert(gone.clone()) {
                continue;
            }
            list.push(Deleted { path, commit: commit.clone(), time, meta: meta.clone() });
        }
        if list.len() >= limit {
            break;
        }
    }
    list.truncate(limit);
    Ok(list)
}

/// The last text of a deleted note: the file as it was just before the commit that deleted it.
pub fn deleted_content(repo: &Repo, d: &Deleted) -> Result<Vec<u8>, String> {
    content_at(repo, &format!("{}^", d.commit), &d.path)
}

/// `dir/Name (restored).md`, then `Name (restored 2).md` while the name is taken (SYNC-040).
pub fn restored_name(path: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    super::numbered_name(path, "restored", taken)
}

/// When snapshots are due (SYNC-033): a pure schedule, with the time passed in (seconds, any
/// origin), so tests need no sleeps.
#[derive(Debug, Default)]
pub struct Schedule {
    /// Saved and not yet snapshotted: the time of the last save.
    pending: BTreeMap<PathBuf, u64>,
    /// When each path was last snapshotted.
    last: HashMap<PathBuf, u64>,
    /// Paths whose last tab closed.
    closed: HashSet<PathBuf>,
}

impl Schedule {
    /// A note or drawing was saved.
    pub fn saved(&mut self, path: &Path, now: u64) {
        self.pending.insert(path.to_path_buf(), now);
    }

    /// The last tab of this note closed: its saved changes are due now.
    pub fn closed(&mut self, path: &Path) {
        if self.pending.contains_key(path) {
            self.closed.insert(path.to_path_buf());
        }
    }

    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Take what is due now, with the reason: not snapshotted for the coalesce window (or never),
    /// or no change for the idle time (`editing` says that its buffer has unsaved changes), or
    /// closed.
    pub fn due(&mut self, now: u64, cfg: &HistoryConfig, editing: impl Fn(&Path) -> bool) -> Vec<(PathBuf, Reason)> {
        let mut out = vec![];
        for (path, saved) in &self.pending {
            let window = self.last.get(path).is_none_or(|t| now.saturating_sub(*t) >= cfg.coalesce_secs());
            let reason = if self.closed.contains(path) {
                Some(Reason::Close)
            } else if window {
                Some(Reason::Save)
            } else if !editing(path) && now.saturating_sub(*saved) >= cfg.idle_secs() {
                Some(Reason::Idle)
            } else {
                None
            };
            if let Some(r) = reason {
                out.push((path.clone(), r));
            }
        }
        for (p, _) in &out {
            self.pending.remove(p);
            self.closed.remove(p);
            self.last.insert(p.clone(), now);
        }
        out
    }

    /// A snapshot that could not run: ask for these paths again (their time of the last snapshot is undone).
    pub fn again(&mut self, paths: &[PathBuf], now: u64) {
        for p in paths {
            self.last.remove(p);
            self.pending.entry(p.clone()).or_insert(now);
        }
    }

    /// Everything that is pending, at window close.
    pub fn take_all(&mut self) -> Vec<PathBuf> {
        self.closed.clear();
        std::mem::take(&mut self.pending).into_keys().collect()
    }

    /// Take the pending paths at or below `prefix` (a note or a folder).
    pub fn take_under(&mut self, prefix: &Path) -> Vec<PathBuf> {
        let under: Vec<PathBuf> = self.pending.keys().filter(|p| p.starts_with(prefix)).cloned().collect();
        for p in &under {
            self.forget(p);
        }
        under
    }

    /// Forget one path (it was snapshotted by hand, or it is gone).
    pub fn forget(&mut self, path: &Path) {
        self.pending.remove(path);
        self.closed.remove(path);
    }

    /// Seconds until something becomes due (0: now); None: nothing is pending.
    pub fn next_in(&self, now: u64, cfg: &HistoryConfig) -> Option<u64> {
        self.pending
            .iter()
            .map(|(p, saved)| {
                if self.closed.contains(p) {
                    return 0;
                }
                let window = self.last.get(p).map_or(0, |t| (t + cfg.coalesce_secs()).saturating_sub(now));
                window.min((saved + cfg.idle_secs()).saturating_sub(now))
            })
            .min()
    }
}

pub mod thin;

#[cfg(test)]
mod history_tests;
