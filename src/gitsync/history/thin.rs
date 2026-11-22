//! Thinning a local-only file history (SYNC-067…SYNC-071, ADR-0028 amendment of 2026-11-22, #479).
//!
//! A history in the data folder keeps every snapshot of the last day, the newest of each hour up to
//! 30 days, and the newest of each day before that. The rewrite builds a new linear history of the
//! kept commits (`git commit-tree`), moves `refs/heads/main` to it in one compare-and-swap
//! (`git update-ref <new> <old>`) and then lets git forget the old commits. The vault's own `.git`
//! is never rewritten (SYNC-012): [`thin`] refuses a [`Repo`] without a git directory.
//!
//! - [`keep`]`(&[Commit], now) -> Vec<bool>`: pure, which commits stay.
//! - [`plan`]`(&Repo, now)`, [`apply`]`(&Repo, &Plan)`: the new history (nothing is referenced yet),
//!   and the swap and the clean-up.
//! - [`thin`] and [`run_if_due`]: both under the tree lock; the second at most once a day ([`due`]).

use super::{parse_message, Reason, Repo};
use crate::gitsync::TreeLock;
use std::collections::HashSet;
use std::ffi::OsString;
use std::time::Duration;

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
/// Where the hourly versions end and the daily ones start.
const MONTH: i64 = 30 * DAY;
/// The file of a git directory that holds the unix time of the last run (next to `worktree`).
pub(crate) const STAMP_FILE: &str = "thinned";
/// How long thinning waits before it runs again (SYNC-070).
pub const INTERVAL_SECS: u64 = 24 * 3600;

/// What [`keep`] needs to know about a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    /// Seconds since 1970 (the committer date).
    pub time: i64,
    pub reason: Option<Reason>,
    /// The commit deletes a path.
    pub deletes: bool,
}

/// Which of `commits` (newest first) stay, as flags in the same order (SYNC-067, SYNC-068).
///
/// By age (UTC buckets): younger than 24 h all; then the newest of each hour up to 30 days; then
/// the newest of each day. Always: the newest and the oldest commit, snapshots of the reasons
/// restore, delete and rename, every commit that deletes a path, and the commit before one that
/// deletes (the deleted note's last text, SYNC-040).
pub fn keep(commits: &[Commit], now: i64) -> Vec<bool> {
    let mut kept = vec![false; commits.len()];
    let mut seen: HashSet<(bool, i64)> = HashSet::new();
    for (i, c) in commits.iter().enumerate() {
        let age = now - c.time;
        if age < DAY {
            kept[i] = true;
        } else if age < MONTH {
            kept[i] |= seen.insert((false, c.time.div_euclid(HOUR)));
        } else {
            kept[i] |= seen.insert((true, c.time.div_euclid(DAY)));
        }
        let deleting = c.deletes || c.reason == Some(Reason::Delete);
        if deleting || matches!(c.reason, Some(Reason::Restore | Reason::Rename)) {
            kept[i] = true;
        }
        if deleting {
            if let Some(parent) = kept.get_mut(i + 1) {
                *parent = true;
            }
        }
    }
    if let Some(first) = kept.first_mut() {
        *first = true;
    }
    if let Some(last) = kept.last_mut() {
        *last = true;
    }
    kept
}

/// What a run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A vault with its own `.git`: never thinned (SYNC-069).
    Skipped,
    /// The last run was less than a day ago.
    NotDue,
    /// Every commit stays.
    Unchanged,
    /// The history now has `kept` commits; `dropped` are gone.
    Thinned { kept: usize, dropped: usize },
    /// Not now (the tree lock): ask again later.
    Busy(String),
}

/// A new history that is built and not yet referenced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub old_tip: String,
    pub new_tip: String,
    pub kept: usize,
    pub dropped: usize,
}

/// The seconds of the last run, if there was one.
pub fn last_thinned(repo: &Repo) -> Option<u64> {
    let gd = repo.git_dir.as_ref()?;
    crate::fsutil::read_to_string(gd.join(STAMP_FILE)).ok()?.trim().parse().ok()
}

/// Is a run due (SYNC-070): never run, or the last run is a day old.
pub fn due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|t| now.saturating_sub(t) >= INTERVAL_SECS)
}

/// The git directory, or the refusal for a vault's own repository (SYNC-069).
fn local(repo: &Repo) -> Result<&std::path::PathBuf, String> {
    repo.git_dir.as_ref().ok_or_else(|| "the history of a vault's own git repository is never thinned".to_string())
}

/// One commit of the old history.
struct Old {
    commit: Commit,
    tree: String,
    author: [String; 3],
    committer: [String; 3],
    message: String,
}

const REC: char = '\u{1e}';
const FLD: char = '\u{1f}';

fn read_history(repo: &Repo, tip: &str) -> Result<Vec<Old>, String> {
    let fmt = "--format=%x1e%H%x1f%T%x1f%ct%x1f%an%x1f%ae%x1f%ad%x1f%cn%x1f%ce%x1f%cd%x1f%B";
    let out = repo.ok(&osv!["log", "--first-parent", "--date=raw", fmt, tip])?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut list = vec![];
    for rec in text.split(REC).skip(1) {
        let f: Vec<&str> = rec.splitn(10, FLD).collect();
        if f.len() < 10 {
            return Err("git log gave a commit that could not be read".into());
        }
        let message = f[9].strip_suffix('\n').unwrap_or(f[9]).to_string();
        list.push(Old {
            commit: Commit { id: f[0].to_string(), time: f[2].parse().map_err(|_| "a commit has no time".to_string())?, reason: parse_message(&message).reason, deletes: false },
            tree: f[1].to_string(),
            author: [f[3].into(), f[4].into(), f[5].into()],
            committer: [f[6].into(), f[7].into(), f[8].into()],
            message,
        });
    }
    // the commits that delete a path: any path, whoever made the commit
    let del = repo.ok(&osv!["log", "--first-parent", "--no-renames", "--diff-filter=D", "--name-only", "--format=%x1e%H", tip])?;
    let del = String::from_utf8_lossy(&del.stdout).into_owned();
    let deleting: HashSet<&str> = del.split(REC).skip(1).filter_map(|r| r.split_once('\n').filter(|(_, names)| !names.trim().is_empty()).map(|(id, _)| id.trim())).collect();
    for o in &mut list {
        o.commit.deletes = deleting.contains(o.commit.id.as_str());
    }
    Ok(list)
}

/// Build the new history of the kept commits of the repository (SYNC-069). Nothing refers to it yet:
/// the objects are unreachable until [`apply`] moves the branch. `None`: nothing would go.
pub fn plan(repo: &Repo, now: i64) -> Result<Option<Plan>, String> {
    let gd = local(repo)?;
    if !gd.join("HEAD").exists() {
        return Ok(None);
    }
    if repo.ok(&osv!["symbolic-ref", "HEAD"])?.text() != "refs/heads/main" {
        return Err("the history is not on the branch main".into());
    }
    let old_tip = repo.ok(&osv!["rev-parse", "--verify", "--quiet", "refs/heads/main"]);
    let Ok(old_tip) = old_tip.map(|o| o.text()) else { return Ok(None) }; // no commit yet
    let old = read_history(repo, &old_tip)?;
    let flags = keep(&old.iter().map(|o| o.commit.clone()).collect::<Vec<_>>(), now);
    let kept = flags.iter().filter(|k| **k).count();
    if kept == old.len() {
        return Ok(None);
    }
    let mut parent: Option<String> = None;
    for (o, _) in old.iter().zip(&flags).rev().filter(|(_, k)| **k) {
        let mut git = repo.git.clone();
        for (k, v) in [("AUTHOR", &o.author), ("COMMITTER", &o.committer)] {
            git.env.push((format!("GIT_{k}_NAME").into(), v[0].clone().into()));
            git.env.push((format!("GIT_{k}_EMAIL").into(), v[1].clone().into()));
            git.env.push((format!("GIT_{k}_DATE").into(), v[2].clone().into()));
        }
        let scratch = Repo { git, vault: repo.vault.clone(), git_dir: repo.git_dir.clone() };
        let mut a = osv!["-c", "commit.gpgsign=false", "commit-tree", o.tree.as_str()].to_vec();
        if let Some(p) = &parent {
            a.extend(osv!["-p", p.as_str()]);
        }
        a.extend(osv!["-m", o.message.as_str()]);
        parent = Some(scratch.ok(&a)?.text());
    }
    let new_tip: String = parent.ok_or("no commit was kept")?;
    Ok(Some(Plan { old_tip, new_tip, kept, dropped: old.len() - kept }))
}

/// Move `main` to the new history, only when it is still where the plan found it, and clean up
/// (SYNC-069). When the swap fails the history is as it was.
pub fn apply(repo: &Repo, plan: &Plan) -> Result<(), String> {
    local(repo)?;
    repo.ok(&osv!["update-ref", "-m", "Shale: thin the history", "refs/heads/main", plan.new_tip.as_str(), plan.old_tip.as_str()])?;
    // the old commits are unreachable now; the reflog would keep them
    let clean = repo.ok(&osv!["reflog", "expire", "--expire=now", "--all"]).and_then(|_| repo.ok(&osv!["gc", "--prune=now", "--quiet"]));
    clean.map(|_| ()).map_err(|e| format!("the history was thinned, but old data could not be removed: {e}"))
}

fn write_stamp(repo: &Repo, now: u64) {
    if let Some(gd) = &repo.git_dir {
        // a stamp that cannot be written only makes the next run come sooner
        let _ = crate::fsutil::write_atomic(&gd.join(STAMP_FILE), format!("{now}\n").as_bytes());
    }
}

/// Thin the history now (SYNC-067…SYNC-069): refuses a vault's own repository, holds the tree lock
/// for the time it runs (`wait` for it, then [`Outcome::Busy`]), and writes the time of the run.
pub fn thin(repo: &Repo, tree: &TreeLock, wait: Duration, now: u64) -> Result<Outcome, String> {
    local(repo)?;
    let Some(_tree) = tree.hold_for(wait) else { return Ok(Outcome::Busy("the vault's files are in use".into())) };
    let _flow = tracing::info_span!("flow.history_thin", peer = "git").entered();
    write_stamp(repo, now);
    match plan(repo, i64::try_from(now).unwrap_or(i64::MAX))? {
        None => Ok(Outcome::Unchanged),
        Some(p) => {
            apply(repo, &p)?;
            Ok(Outcome::Thinned { kept: p.kept, dropped: p.dropped })
        }
    }
}

/// [`thin`] when a run is due (SYNC-070); a vault's own repository gives [`Outcome::Skipped`].
pub fn run_if_due(repo: &Repo, tree: &TreeLock, wait: Duration, now: u64) -> Result<Outcome, String> {
    if repo.git_dir.is_none() {
        return Ok(Outcome::Skipped);
    }
    if !due(last_thinned(repo), now) {
        return Ok(Outcome::NotDue);
    }
    thin(repo, tree, wait, now)
}

#[cfg(test)]
mod thin_tests;
