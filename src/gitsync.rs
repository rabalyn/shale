//! Integrated git sync of a vault (ADR-0008, SYNC-001…SYNC-012).
//!
//! Runs the system `git` with argument lists (never a shell). One sync cycle commits every
//! change, fetches, fast-forwards or merges, turns conflicts into conflict copies (never
//! conflict markers, never a lost side) and pushes, never with force.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The remote Shale syncs with.
pub const REMOTE: &str = "origin";

/// Session state that may be lost (ADR-0005): on a conflict the local version wins, no copy.
pub const STATE_PATH: &str = ".shale/state.json";

/// How git is started.
#[derive(Clone, Debug)]
pub struct Git {
    pub program: PathBuf,
    /// Extra environment (tests use it for a hermetic git configuration).
    pub env: Vec<(OsString, OsString)>,
}

impl Default for Git {
    fn default() -> Self {
        Git { program: PathBuf::from("git"), env: vec![] }
    }
}

/// The result of a command that ran.
struct Out {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

impl Out {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

impl Git {
    /// `git -C <dir> args…` with stdin closed, no prompts and no inherited repository variables.
    fn command(&self, dir: &Path, args: &[&OsStr]) -> Command {
        let mut c = Command::new(&self.program);
        c.arg("-C").arg(dir).args(args);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR", "GIT_NAMESPACE"] {
            c.env_remove(v);
        }
        c.env("GIT_TERMINAL_PROMPT", "0");
        c.env("GIT_MERGE_AUTOEDIT", "no");
        c.env("LC_ALL", "C");
        for (k, v) in &self.env {
            c.env(k, v);
        }
        c
    }

    fn run(&self, dir: &Path, args: &[&OsStr]) -> Result<Out, String> {
        let o: Output = self.command(dir, args).output().map_err(|e| format!("git could not be started ({}): {e}", self.program.display()))?;
        Ok(Out { ok: o.status.success(), stdout: o.stdout, stderr: hide_passwords(String::from_utf8_lossy(&o.stderr).trim()) })
    }

    /// Run and require success; the error is git's own message.
    fn ok(&self, dir: &Path, args: &[&OsStr]) -> Result<Out, String> {
        let o = self.run(dir, args)?;
        if o.ok {
            Ok(o)
        } else {
            let what = args.iter().take(3).map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
            Err(if o.stderr.is_empty() { format!("git {what} failed") } else { format!("git {what}: {}", o.stderr) })
        }
    }

    /// The installed git's version line, or None without git (SYNC-003).
    pub fn version(&self) -> Option<String> {
        let o = Command::new(&self.program).arg("--version").stdin(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }
}

/// Short way to build argument lists.
macro_rules! args {
    ($($a:expr),* $(,)?) => { &[$(AsRef::<OsStr>::as_ref(&$a)),*] };
}

/// The vault itself is a git repository (a repository in a parent folder does not count).
pub fn is_repo(vault: &Path) -> bool {
    vault.join(".git").exists()
}

/// The URL of `origin`, if there is one.
pub fn remote_url(git: &Git, vault: &Path) -> Option<String> {
    if !is_repo(vault) {
        return None;
    }
    let o = git.run(vault, args!["remote", "get-url", REMOTE]).ok()?;
    o.ok.then(|| o.text()).filter(|u| !u.is_empty())
}

/// Replace the password of every `scheme://user:password@host` in a text with `***`, so an address
/// set outside Shale never shows its password in the settings or in an error message (SEC-010).
pub fn hide_passwords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail.find(|c: char| matches!(c, '/' | '?' | '#' | '\\' | '\'' | '"') || c.is_whitespace()).unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        match authority.rsplit_once('@') {
            Some((userinfo, host)) if userinfo.contains(':') => {
                let user = userinfo.split(':').next().unwrap_or("");
                out.push_str(&format!("{user}:***@{host}"));
            }
            _ => out.push_str(authority),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Check a remote URL typed by the user (SYNC-004).
pub fn check_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("The URL is empty".into());
    }
    if url.starts_with('-') {
        return Err("A URL can't start with '-'".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("A URL can't contain spaces or control characters".into());
    }
    let lower = url.to_ascii_lowercase();
    if let Some((scheme, rest)) = lower.split_once("://") {
        if !["https", "http", "ssh", "git", "file"].contains(&scheme) {
            return Err("Use an https://, ssh:// or git@host:path URL, or an absolute folder path".into());
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let (userinfo, host_port) = authority.rsplit_once('@').unwrap_or(("", authority));
        if userinfo.contains(':') {
            return Err("The address has a password in it. Remove it and use a git credential helper or an SSH key instead; a password in the address is stored in plain text.".into());
        }
        if scheme == "http" || scheme == "git" {
            let host = if host_port.starts_with('[') { host_port.split_inclusive(']').next().unwrap_or("") } else { host_port.split(':').next().unwrap_or("") };
            if !["localhost", "127.0.0.1", "[::1]"].contains(&host) {
                return Err(format!("{scheme}:// sends your notes unencrypted. Use https:// or ssh:// instead (only this computer may use {scheme}://)."));
            }
        }
        return Ok(());
    }
    // remote helpers (`ext::sh -c …`, `fd::…`) can run programs
    if url.contains("::") {
        return Err("Remote helper URLs (like ext::) are not allowed".into());
    }
    if Path::new(url).is_absolute() {
        return Ok(());
    }
    // scp-like: [user@]host:path
    if let Some((host, path)) = url.split_once(':') {
        if !host.contains('@') && path.split('/').next().is_some_and(|first| first.contains('@')) {
            return Err("The address has a password in it. Remove it and use a git credential helper or an SSH key instead.".into());
        }
        let host_ok = !host.is_empty() && !host.contains('/') && host.chars().all(|c| c.is_ascii_alphanumeric() || "@.-_[]".contains(c));
        if host_ok && !path.is_empty() {
            return Ok(());
        }
    }
    Err("Use an https://, ssh:// or git@host:path URL, or an absolute folder path".into())
}

/// Make the vault a repository if needed and point `origin` at `url` (SYNC-004).
pub fn connect(git: &Git, vault: &Path, url: &str) -> Result<(), String> {
    check_url(url)?;
    let url = url.trim();
    if !is_repo(vault) {
        git.ok(vault, args!["init", "--quiet"])?;
    }
    let has_origin = git.run(vault, args!["remote", "get-url", REMOTE])?.ok;
    if has_origin {
        git.ok(vault, args!["remote", "set-url", REMOTE, url])?;
    } else {
        git.ok(vault, args!["remote", "add", REMOTE, url])?;
    }
    Ok(())
}

/// What a sync should write into commit messages and conflict copy names.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub commit_prefix: String,
    pub commit_suffix: String,
    /// This computer's name, for commit messages.
    pub host: String,
    /// Local time stamp for conflict copies, "YYYY-MM-DD HHMM".
    pub stamp: String,
}

impl Options {
    fn message(&self, body: &str) -> String {
        format!("{}{body}{}", self.commit_prefix, self.commit_suffix)
    }
}

/// What a sync did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Files in the local commit.
    pub committed: usize,
    /// Vault-relative files the sync changed in the working tree (merged in from the remote,
    /// conflict copies included).
    pub changed: Vec<PathBuf>,
    /// (file, conflict copy of the local version)
    pub conflicts: Vec<(PathBuf, PathBuf)>,
    /// There is a remote.
    pub remote: bool,
    /// The local branch is now on the remote.
    pub pushed: bool,
}

impl Report {
    /// One line for the user.
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if self.committed > 0 {
            parts.push(format!("{} file(s) committed", self.committed));
        }
        let incoming = self.changed.len() - self.conflicts.len();
        if incoming > 0 {
            parts.push(format!("{incoming} file(s) updated from the remote"));
        }
        if !self.conflicts.is_empty() {
            parts.push(format!("{} conflict copy/copies made", self.conflicts.len()));
        }
        if !self.remote {
            parts.push("no remote (local history only)".into());
        } else if self.pushed {
            parts.push("up to date with the remote".into());
        }
        if parts.is_empty() {
            "nothing to do".into()
        } else {
            parts.join(", ")
        }
    }
}

/// The lock on the vault's working tree. The sync holds it while git changes files; the window
/// holds it while it writes notes, so a save never lands inside a merge (SYNC-013). The guard
/// owns its handle, so a window function can keep it across `&mut self` calls.
#[derive(Clone, Default)]
pub struct TreeLock(Arc<(Mutex<bool>, Condvar)>);

/// Releases the lock when dropped, also when the holder panics.
pub struct TreeGuard(TreeLock);

impl TreeLock {
    fn state(&self) -> MutexGuard<'_, bool> {
        // the flag is only ever flipped, so a poisoned mutex still holds a true answer
        self.0 .0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The lock if it is free right now (the window never blocks on it).
    pub fn try_hold(&self) -> Option<TreeGuard> {
        let mut held = self.state();
        if *held {
            return None;
        }
        *held = true;
        Some(TreeGuard(self.clone()))
    }

    /// Wait for the lock for at most `limit`.
    pub fn hold_for(&self, limit: Duration) -> Option<TreeGuard> {
        let end = Instant::now() + limit;
        let mut held = self.state();
        while *held {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            held = self.0 .1.wait_timeout(held, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        *held = true;
        Some(TreeGuard(self.clone()))
    }

    /// Wait as long as it takes (the sync job, which has no window to keep alive).
    pub fn hold(&self) -> TreeGuard {
        loop {
            if let Some(g) = self.hold_for(Duration::from_secs(3600)) {
                return g;
            }
        }
    }
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        *self.0.state() = false;
        self.0 .0 .1.notify_all();
    }
}

/// Run one sync cycle (ADR-0008 §4). `tree` is held while files in the vault are changed,
/// so the app cannot save a note meanwhile.
pub fn sync(git: &Git, vault: &Path, opts: &Options, tree: &TreeLock) -> Result<Report, String> {
    if !is_repo(vault) {
        return Err("The vault is not a git repository".into());
    }
    // SYNC-005: never step into someone else's unfinished operation
    for (marker, what) in [("MERGE_HEAD", "a merge"), ("rebase-merge", "a rebase"), ("rebase-apply", "a rebase"), ("CHERRY_PICK_HEAD", "a cherry-pick"), ("REVERT_HEAD", "a revert")] {
        let p = git.ok(vault, args!["rev-parse", "--git-path", marker])?.text();
        if vault.join(&p).exists() {
            return Err(format!("git is in the middle of {what}; finish or abort it in the vault first (sync paused)"));
        }
    }
    let head = git.run(vault, args!["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if !head.ok {
        return Err("HEAD is detached; check out a branch in the vault first (sync paused)".into());
    }
    let mut branch = head.text();
    let ident = identity(git, vault)?;
    let mut report = Report::default();

    // 1. commit everything (SYNC-006)
    git.ok(vault, args!["add", "-A", "--", ".", ":(exclude,glob)**/.shale-tmp-*"])?;
    let staged = git.ok(vault, args!["diff", "--cached", "--name-only", "-z"])?;
    report.committed = staged.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).count();
    if report.committed > 0 {
        let msg = opts.message(&format!("Sync from {}: {} file(s)", opts.host, report.committed));
        commit(git, vault, &ident, &["commit", "--quiet", "-m", &msg])?;
    }

    // 2. with a remote: fetch, merge, push (SYNC-007), a few times if the remote moves on
    if !git.run(vault, args!["remote", "get-url", REMOTE])?.ok {
        return Ok(report);
    }
    report.remote = true;
    for attempt in 0..3 {
        git.ok(vault, args!["fetch", "--quiet", "--no-tags", REMOTE])?;
        if attempt == 0 {
            branch = align_branch(git, vault, &branch)?;
        }
        let local = rev(git, vault, "HEAD")?;
        let remote_ref = format!("refs/remotes/{REMOTE}/{branch}");
        if let Some(theirs) = rev(git, vault, &remote_ref)? {
            if local.as_deref() != Some(theirs.as_str()) {
                integrate(git, vault, opts, &ident, local.as_deref(), &theirs, tree, &mut report)?;
            }
        }
        if rev(git, vault, "HEAD")?.is_none() {
            // an empty repository and an empty remote: nothing to push yet
            return Ok(report);
        }
        let spec = format!("refs/heads/{branch}:refs/heads/{branch}");
        // SYNC-012: never forced
        let push = git.run(vault, args!["push", "--quiet", "--set-upstream", REMOTE, spec])?;
        if push.ok {
            report.pushed = true;
            return Ok(report);
        }
        let rejected = ["rejected", "fetch first", "non-fast-forward"].iter().any(|w| push.stderr.contains(w));
        if !rejected || attempt == 2 {
            return Err(format!("git push: {}", push.stderr));
        }
    }
    Ok(report)
}

/// `-c user.name=… -c user.email=…` when git has no identity configured.
fn identity(git: &Git, vault: &Path) -> Result<Vec<String>, String> {
    let name = git.run(vault, args!["config", "user.name"])?.text();
    let email = git.run(vault, args!["config", "user.email"])?.text();
    let mut out = vec![];
    if name.is_empty() {
        out.extend(["-c".into(), "user.name=Shale".into()]);
    }
    if email.is_empty() {
        out.extend(["-c".into(), "user.email=shale@localhost".into()]);
    }
    Ok(out)
}

fn commit(git: &Git, vault: &Path, ident: &[String], args: &[&str]) -> Result<Out, String> {
    let all: Vec<&OsStr> = ident.iter().map(OsStr::new).chain(args.iter().map(OsStr::new)).collect();
    git.ok(vault, &all)
}

/// The commit a ref points at, None if it does not exist (an unborn branch, a missing remote branch).
fn rev(git: &Git, vault: &Path, r: &str) -> Result<Option<String>, String> {
    let spec = format!("{r}^{{commit}}");
    let o = git.run(vault, args!["rev-parse", "--verify", "--quiet", spec])?;
    Ok(o.ok.then(|| o.text()))
}

/// A fresh repository connected to a remote whose branch has another name (`master` here,
/// `main` there): follow the remote's default branch instead of pushing a second branch.
fn align_branch(git: &Git, vault: &Path, branch: &str) -> Result<String, String> {
    if rev(git, vault, &format!("refs/remotes/{REMOTE}/{branch}"))?.is_some() {
        return Ok(branch.to_string());
    }
    let o = git.run(vault, args!["ls-remote", "--symref", REMOTE, "HEAD"])?;
    let theirs = o.text().lines().find_map(|l| l.strip_prefix("ref: refs/heads/").and_then(|r| r.split('\t').next()).map(str::to_string));
    let Some(theirs) = theirs.filter(|t| t != branch) else { return Ok(branch.to_string()) };
    if rev(git, vault, &format!("refs/remotes/{REMOTE}/{theirs}"))?.is_none() || rev(git, vault, &format!("refs/heads/{theirs}"))?.is_some() {
        return Ok(branch.to_string());
    }
    if rev(git, vault, "HEAD")?.is_none() {
        git.ok(vault, args!["symbolic-ref", "HEAD", format!("refs/heads/{theirs}")])?;
    } else {
        git.ok(vault, args!["branch", "-m", branch, &theirs])?;
    }
    Ok(theirs)
}

/// Bring the remote commit `theirs` into HEAD: fast-forward or merge, conflicts as copies.
#[allow(clippy::too_many_arguments)]
fn integrate(git: &Git, vault: &Path, opts: &Options, ident: &[String], local: Option<&str>, theirs: &str, tree: &TreeLock, report: &mut Report) -> Result<(), String> {
    if let Some(l) = local {
        if git.run(vault, args!["merge-base", "--is-ancestor", theirs, l])?.ok {
            return Ok(()); // the remote is behind: just push
        }
    }
    let _tree = tree.hold();
    let fast = match local {
        None => true,
        Some(l) => git.run(vault, args!["merge-base", "--is-ancestor", l, theirs])?.ok,
    };
    if fast {
        git.ok(vault, args!["merge", "--ff-only", "--quiet", theirs])?;
    } else {
        let msg = opts.message(&format!("Sync from {}: merge", opts.host));
        let all: Vec<&OsStr> = ident.iter().map(OsStr::new).chain(["merge", "--no-ff", "--no-edit", "--quiet", "--allow-unrelated-histories", "-m", msg.as_str(), theirs].into_iter().map(OsStr::new)).collect();
        let m = git.run(vault, &all)?;
        if !m.ok {
            let in_merge = vault.join(git.ok(vault, args!["rev-parse", "--git-path", "MERGE_HEAD"])?.text()).exists();
            if !in_merge {
                // refused before touching anything (e.g. a changed file it would overwrite)
                return Err(format!("git merge: {}", m.stderr));
            }
            if let Err(e) = resolve(git, vault, opts, ident, report) {
                let _ = git.run(vault, args!["merge", "--abort"]);
                return Err(format!("{e} (the merge was undone; local changes are in the last commit)"));
            }
        }
    }
    let new = rev(git, vault, "HEAD")?.ok_or("HEAD vanished after merging")?;
    let changed = match local {
        Some(l) => git.ok(vault, args!["diff", "--name-only", "-z", "--no-renames", l, &new])?,
        None => git.ok(vault, args!["ls-tree", "-r", "--name-only", "-z", &new])?,
    };
    report.changed = changed.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned())).collect();
    Ok(())
}

/// Resolve every conflicted path without markers (SYNC-008…SYNC-010) and commit the merge.
fn resolve(git: &Git, vault: &Path, opts: &Options, ident: &[String], report: &mut Report) -> Result<(), String> {
    // path → stage → (mode, blob)
    let mut unmerged: BTreeMap<String, BTreeMap<u8, (String, String)>> = BTreeMap::new();
    let ls = git.ok(vault, args!["ls-files", "--unmerged", "-z"])?;
    for entry in ls.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let entry = String::from_utf8_lossy(entry);
        let (meta, path) = entry.split_once('\t').ok_or("unexpected git ls-files output")?;
        let mut it = meta.split(' ');
        let (mode, blob, stage) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        let stage: u8 = stage.parse().map_err(|_| "unexpected git ls-files output")?;
        unmerged.entry(path.to_string()).or_default().insert(stage, (mode.to_string(), blob.to_string()));
    }
    if unmerged.is_empty() {
        return Err("git merge failed without conflicts".into());
    }
    let mut taken: Vec<PathBuf> = vec![];
    let mut add: Vec<String> = vec![];
    for (path, stages) in &unmerged {
        if stages.values().any(|(mode, _)| !mode.starts_with("100")) {
            return Err(format!("“{path}” is a link or submodule in conflict; resolve it with git"));
        }
        let blob = |s: u8| -> Result<Option<Vec<u8>>, String> {
            match stages.get(&s) {
                Some((_, b)) => Ok(Some(git.ok(vault, args!["cat-file", "blob", b])?.stdout)),
                None => Ok(None),
            }
        };
        let (ours, theirs) = (blob(2)?, blob(3)?);
        let abs = vault.join(path);
        match (ours, theirs) {
            (Some(o), Some(t)) => {
                if path == STATE_PATH {
                    crate::fsutil::write_atomic(&abs, &o).map_err(|e| e.to_string())?;
                } else {
                    crate::fsutil::write_atomic(&abs, &t).map_err(|e| e.to_string())?;
                    if o != t {
                        let copy = conflict_name(Path::new(path), &opts.stamp, |p| vault.join(p).exists() || taken.iter().any(|x| x == p));
                        crate::fsutil::write_atomic(&vault.join(&copy), &o).map_err(|e| e.to_string())?;
                        add.push(copy.to_string_lossy().into_owned());
                        report.conflicts.push((PathBuf::from(path), copy.clone()));
                        taken.push(copy);
                    }
                }
            }
            // one side deleted it, the other changed it: keep the changed version
            (Some(c), None) | (None, Some(c)) => crate::fsutil::write_atomic(&abs, &c).map_err(|e| e.to_string())?,
            // both deleted (renamed apart): nothing to keep at this path
            (None, None) => {
                git.ok(vault, args!["rm", "--quiet", "--cached", "--ignore-unmatch", "--", format!(":(literal){path}")])?;
                continue;
            }
        }
        add.push(path.clone());
    }
    let specs: Vec<String> = add.iter().map(|p| format!(":(literal){p}")).collect();
    let mut a: Vec<&OsStr> = vec![OsStr::new("add"), OsStr::new("--")];
    a.extend(specs.iter().map(OsStr::new));
    git.ok(vault, &a)?;
    let n = report.conflicts.len();
    let msg = opts.message(&format!("Sync from {}: merge, {n} conflict copy/copies", opts.host));
    commit(git, vault, ident, &["commit", "--quiet", "-m", &msg])?;
    Ok(())
}

/// `dir/Name (conflict 2026-11-14 1530).md`; `.excalidraw.md` stays whole; a taken name gets
/// a number (`… 1530 2).md`).
pub fn conflict_name(path: &Path, stamp: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = if name.to_ascii_lowercase().ends_with(".excalidraw.md") {
        let i = name.len() - ".excalidraw.md".len();
        (name[..i].to_string(), name[i..].to_string())
    } else {
        match name.rfind('.') {
            Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
            _ => (name.clone(), String::new()),
        }
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut n = 1;
    loop {
        let num = if n == 1 { String::new() } else { format!(" {n}") };
        let p = dir.join(format!("{stem} (conflict {stamp}{num}){ext}"));
        if !taken(&p) {
            return p;
        }
        n += 1;
    }
}

/// "YYYY-MM-DD HHMM" in local time, for conflict copies.
pub fn stamp_now() -> String {
    let (days, sod) = crate::cal_time::utc_to_local(crate::cal_time::now());
    let (y, m, d) = crate::cal_time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}{:02}", sod / 3600, sod % 3600 / 60)
}

/// This computer's name, for commit messages.
pub fn host_name() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: the buffer is valid for its length; gethostname writes at most that much.
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            let h = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !h.is_empty() {
                return h;
            }
        }
    }
    // Windows keeps the computer's name in the environment of every program
    #[cfg(windows)]
    if let Ok(h) = std::env::var("COMPUTERNAME") {
        let h = h.trim().to_string();
        if !h.is_empty() {
            return h;
        }
    }
    "this computer".into()
}

#[cfg(test)]
mod tests;
