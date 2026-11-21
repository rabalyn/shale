//! A made-up history for screenshots and clips (HIST-018): `SHALE_DEMO_HISTORY=<folder>`.

use crate::app::ShaleApp;
use crate::gitsync::history::{self, Reason, Request};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// One step of the made-up history: the folder `v1`, `v2`, … (or `v3-sync-laptop`: a version that came
/// from another device by the sync) and the files in it.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub folder: PathBuf,
    pub reason: Reason,
    /// The device's name, when it is another one.
    pub device: Option<String>,
}

/// The steps in `dir`, in the order of their numbers.
pub fn steps(dir: &Path) -> Vec<Step> {
    let mut found: Vec<(u32, Step)> = vec![];
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix('v') else { continue };
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let Ok(n) = rest[..digits].parse::<u32>() else { continue };
        let (reason, device) = match rest[digits..].strip_prefix("-sync-") {
            Some(d) if !d.is_empty() => (Reason::Sync, Some(d.to_string())),
            _ => (Reason::Save, None),
        };
        found.push((n, Step { folder: entry.path(), reason, device }));
    }
    found.sort_by_key(|(n, _)| *n);
    found.into_iter().map(|(_, s)| s).collect()
}

/// Copy the files of `from` over `to`; the vault-relative paths of what was copied.
fn copy_over(from: &Path, to: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from.join(rel))? {
        let entry = entry?;
        let path = rel.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_over(from, to, &path, out)?;
        } else {
            let target = to.join(&path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), target)?;
            out.push(path);
        }
    }
    Ok(())
}

impl ShaleApp {
    /// Make the history of the open vault from the steps in `dir`: each step is copied over the vault and
    /// versioned, one hour after the one before, the last one an hour before "now" (the frozen clock of a
    /// screenshot). Runs once, before the first frame.
    pub(crate) fn history_demo(&mut self, dir: &Path) -> Result<(), String> {
        let Some((git, root, hist, opts)) = self.history_job_data() else { return Err("no vault".into()) };
        let steps = steps(dir);
        let now = crate::calendar::time::now();
        let tree = self.sync.tree.clone();
        for (i, step) in steps.iter().enumerate() {
            let at = now - 3600 * (steps.len() - i) as i64;
            let mut git = git.clone();
            git.env.extend([("GIT_COMMITTER_DATE", format!("{at} +0000")), ("GIT_AUTHOR_DATE", format!("{at} +0000"))].map(|(k, v)| (OsString::from(k), OsString::from(v))));
            let repo = history::locate(&git, &root, &hist)?;
            let mut paths = vec![];
            copy_over(&step.folder, &root, Path::new(""), &mut paths).map_err(|e| e.to_string())?;
            let mut opts = opts.clone();
            if let Some(d) = &step.device {
                opts.host = d.clone();
            }
            let req = if i == 0 { Request::new(Reason::Initial, vec![]) } else { Request::new(step.reason, paths) };
            history::snapshot(&repo, &opts, &req, &tree, std::time::Duration::from_secs(10))?;
        }
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
        self.history.demo = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: HIST-018
    #[test]
    fn hist_018_the_steps_are_read_in_the_order_of_their_numbers_with_their_device() {
        let dir = std::env::temp_dir().join(format!("shale-demo-steps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["v10", "v2", "v1", "v3-sync-laptop", "other", "vx"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let got: Vec<(String, Reason, Option<String>)> = steps(&dir).into_iter().map(|s| (s.folder.file_name().unwrap().to_string_lossy().into_owned(), s.reason, s.device)).collect();
        assert_eq!(got, [("v1".into(), Reason::Save, None), ("v2".into(), Reason::Save, None), ("v3-sync-laptop".into(), Reason::Sync, Some("laptop".into())), ("v10".into(), Reason::Save, None)]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
