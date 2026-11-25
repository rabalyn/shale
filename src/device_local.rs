//! Device-local state of a vault in a folder another app syncs (NET-016, rabalyn/shale#476).
//!
//! `state.json` (open tabs, panes) and the undo histories belong to one computer. In a vault that
//! a cloud client or a network share carries, two computers would overwrite each other's copy and
//! leave `*.sync-conflict-*` files behind. So for such a vault they live outside it, in
//! `<data folder>/shale/vaults/<vault id>/`, and everything else of `.shale/` (the settings, the
//! tag words) stays in the vault and travels. [`local_dir`] says where the files are for a vault;
//! a vault that is not detected as synced keeps them in `<vault>/.shale/` as before.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Where the local state of the vaults that were placed outside is.
static OUTSIDE: Mutex<Option<HashMap<PathBuf, PathBuf>>> = Mutex::new(None);

/// The prefix of the temporary files of a save (NET-001).
const TMP_PREFIX: &str = ".shale-tmp-";
/// The part of a Syncthing conflict copy's name between the file's name and its extension.
const CONFLICT: &str = ".sync-conflict-";
/// The file in the outside folder that says the files of the vault were moved out.
const MIGRATED: &str = ".migrated";
/// A temporary file younger than this may belong to a save that is still running.
const STALE: Duration = Duration::from_secs(24 * 3600);

/// The folder that holds `state.json` and `undo/` of the vault at `root`.
pub fn local_dir(root: &Path) -> PathBuf {
    let map = OUTSIDE.lock().unwrap_or_else(|e| e.into_inner());
    map.as_ref().and_then(|m| m.get(root)).cloned().unwrap_or_else(|| root.join(".shale"))
}

fn register(root: &Path, dir: Option<PathBuf>) {
    let mut map = OUTSIDE.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    match dir {
        Some(d) => map.insert(root.to_path_buf(), d),
        None => map.remove(root),
    };
}

/// `<data folder>/shale/vaults`: the parent of the folders of the vaults placed outside.
pub fn default_base() -> PathBuf {
    crate::userdirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("vaults")
}

/// Decide where the local state of the vault at `root` lives, before it is loaded. A vault
/// found synced (`synced`) gets an ID (`.shale/history-id`, SYNC-031) and its state goes to
/// `<base>/<id>/`; what an earlier version left in the vault is moved there. Anything that fails
/// leaves the state in the vault and is returned as a message.
pub fn place(root: &Path, synced: bool, base: &Path) -> Option<String> {
    if !synced {
        register(root, None);
        return None;
    }
    let id = match crate::gitsync::history::ensure_vault_id(root) {
        Ok(i) => i,
        Err(e) => {
            register(root, None);
            return Some(format!("Could not keep this vault's local state outside the synced folder: {e}"));
        }
    };
    let dir = base.join(id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        register(root, None);
        return Some(format!("Could not make {}: {e}", dir.display()));
    }
    // only the first placement on this device moves: later, a state.json in the vault belongs to another device
    let marker = dir.join(MIGRATED);
    let moved = if marker.exists() { Ok(()) } else { migrate(&root.join(".shale"), &dir).and_then(|()| crate::fsutil::write_atomic(&marker, b"").map_err(|e| format!("could not write {}: {e}", marker.display()))) };
    register(root, Some(dir));
    moved.err()
}

/// Move `state.json` and the undo histories from `from` (`<vault>/.shale`) to `to`. A file that
/// `to` has already stays, and the copy in the vault is dropped; nothing else is touched.
fn migrate(from: &Path, to: &Path) -> Result<(), String> {
    let mut first_error = None;
    let mut one = |src: &Path, dst: &Path| {
        let r = if dst.exists() { Ok(()) } else { crate::fsutil::read(src).and_then(|b| crate::fsutil::write_atomic(dst, &b)) };
        if let Err(e) = r.and_then(|()| std::fs::remove_file(src)) {
            first_error.get_or_insert_with(|| format!("could not move {}: {e}", src.display()));
        }
    };
    let state = from.join(crate::vault_state::STATE_FILE);
    if state.is_file() {
        one(&state, &to.join(crate::vault_state::STATE_FILE));
    }
    if let Ok(rd) = std::fs::read_dir(from.join("undo")) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_file()) {
                one(&e.path(), &to.join("undo").join(e.file_name()));
            }
        }
        let _ = std::fs::remove_dir(from.join("undo"));
    }
    first_error.map_or(Ok(()), Err)
}

/// What the sweep removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// stale `.shale-tmp-*` files of a crashed save
    pub temp: usize,
    /// conflict copies of the device-local files
    pub conflicts: usize,
}

/// Remove what crashes and sync clients left (NET-016), best effort. Only these are touched:
/// - a `.shale-tmp-*` file (NET-001) anywhere in the vault (`.git` excepted) that is older than a
///   day at `now`;
/// - a `*.sync-conflict-*` copy of `state.json`, and any conflict copy in the undo folder, in the
///   local state folder of the vault (in `.shale/` or outside, see [`local_dir`]).
///
/// Notes are never touched, and neither is a conflict copy of the settings or of the tag words:
/// those hold the other device's change.
pub fn sweep(root: &Path, now: SystemTime) -> Swept {
    let mut out = Swept::default();
    sweep_temp(root, now, &mut out);
    let local = local_dir(root);
    let mut dirs = vec![root.join(".shale")];
    if local != dirs[0] {
        dirs.push(local);
    }
    for dir in dirs {
        remove_conflicts(&dir, |stem| stem == "state", &mut out);
        remove_conflicts(&dir.join("undo"), |_| true, &mut out);
    }
    out
}

fn sweep_temp(dir: &Path, now: SystemTime, out: &mut Swept) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if t.is_dir() {
            if name != ".git" {
                sweep_temp(&e.path(), now, out);
            }
        } else if t.is_file() && name.starts_with(TMP_PREFIX) {
            let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|m| now.duration_since(m).is_ok_and(|age| age >= STALE));
            if old && std::fs::remove_file(e.path()).is_ok() {
                out.temp += 1;
            }
        }
    }
}

/// Remove `<stem>.sync-conflict-…` files of `dir` whose stem `wanted` accepts.
fn remove_conflicts(dir: &Path, wanted: impl Fn(&str) -> bool, out: &mut Swept) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some((stem, _)) = name.split_once(CONFLICT) else { continue };
        if e.file_type().is_ok_and(|t| t.is_file()) && wanted(stem) && std::fs::remove_file(e.path()).is_ok() {
            out.conflicts += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_state::VaultData;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-devlocal-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn set_age(p: &Path, age: Duration) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(SystemTime::now() - age).unwrap();
    }

    // spec: NET-016
    #[test]
    fn a_vault_that_is_not_synced_keeps_its_state_in_the_vault() {
        let (root, base) = (tmp("plain-v"), tmp("plain-b"));
        assert_eq!(place(&root, false, &base), None);
        assert_eq!(local_dir(&root), root.join(".shale"));
        assert!(std::fs::read_dir(&base).unwrap().next().is_none(), "nothing is made outside");
        assert!(!root.join(".shale").exists(), "and nothing in the vault");
    }

    // spec: NET-016
    #[test]
    fn a_synced_vault_keeps_state_and_undo_outside_under_its_id_and_the_settings_stay() {
        let (root, base) = (tmp("synced-v"), tmp("synced-b"));
        assert_eq!(place(&root, true, &base), None);
        let id = crate::gitsync::history::vault_id(&root).expect("the vault got an ID");
        assert_eq!(local_dir(&root), base.join(&id));
        assert_eq!(VaultData::state_path(&root), base.join(&id).join("state.json"));
        let (mut d, problems) = VaultData::load(&root);
        assert!(problems.is_empty());
        d.state.recent_tags = vec!["t".into()];
        d.settings.people_dir = "People".into();
        d.save().unwrap();
        assert!(base.join(&id).join("state.json").is_file());
        assert!(!root.join(".shale").join("state.json").exists(), "the state is not in the vault");
        assert!(root.join(".shale").join("settings.json").is_file(), "the settings travel with the vault");
        // a second open finds the same ID and the same folder
        assert_eq!(place(&root, true, &base), None);
        assert_eq!(local_dir(&root), base.join(&id));
        assert_eq!(VaultData::load(&root).0.state.recent_tags, ["t"]);
    }

    // spec: NET-016
    #[test]
    fn the_state_and_undo_files_an_earlier_version_left_in_the_vault_move_out() {
        let (root, base) = (tmp("migrate-v"), tmp("migrate-b"));
        put(&root.join(".shale/state.json"), r#"{"version":2,"recent_tags":["old"]}"#);
        put(&root.join(".shale/undo/aaaa.json"), "u1");
        put(&root.join(".shale/undo/bbbb.json"), "u2");
        put(&root.join(".shale/settings.json"), "{}");
        put(&root.join("note.md"), "text");
        let id = crate::gitsync::history::ensure_vault_id(&root).unwrap();
        // a file the outside folder has already wins, the one in the vault is dropped
        put(&base.join(&id).join("undo/bbbb.json"), "newer");
        assert_eq!(place(&root, true, &base), None);
        let out = base.join(&id);
        assert!(std::fs::read_to_string(out.join("state.json")).unwrap().contains("old"));
        assert_eq!(std::fs::read_to_string(out.join("undo/aaaa.json")).unwrap(), "u1");
        assert_eq!(std::fs::read_to_string(out.join("undo/bbbb.json")).unwrap(), "newer");
        assert!(!root.join(".shale/state.json").exists());
        assert!(!root.join(".shale/undo").exists(), "the empty folder goes too");
        assert!(root.join(".shale/settings.json").is_file());
        assert_eq!(std::fs::read_to_string(root.join("note.md")).unwrap(), "text");
        assert_eq!(VaultData::load(&root).0.state.recent_tags, ["old"]);
        // later opens leave what appears in the vault alone: it belongs to another device
        put(&root.join(".shale/state.json"), r#"{"version":2,"recent_tags":["other-device"]}"#);
        put(&root.join(".shale/undo/cccc.json"), "u3");
        assert_eq!(place(&root, true, &base), None);
        assert!(std::fs::read_to_string(root.join(".shale/state.json")).unwrap().contains("other-device"));
        assert!(root.join(".shale/undo/cccc.json").is_file());
        assert_eq!(VaultData::load(&root).0.state.recent_tags, ["old"]);
    }

    // spec: NET-016
    #[test]
    fn two_devices_with_the_same_vault_id_keep_separate_state() {
        let (a, b) = (tmp("two-a"), tmp("two-b"));
        let (base_a, base_b) = (tmp("two-base-a"), tmp("two-base-b"));
        assert_eq!(place(&a, true, &base_a), None);
        // the ID file is synced to the other device
        put(&b.join(crate::gitsync::history::ID_FILE), &std::fs::read_to_string(a.join(crate::gitsync::history::ID_FILE)).unwrap());
        assert_eq!(place(&b, true, &base_b), None);
        let (mut da, mut db) = (VaultData::load(&a).0, VaultData::load(&b).0);
        da.state.recent_tags = vec!["from-a".into()];
        db.state.recent_tags = vec!["from-b".into()];
        da.save().unwrap();
        db.save().unwrap();
        assert_eq!(VaultData::load(&a).0.state.recent_tags, ["from-a"]);
        assert_eq!(VaultData::load(&b).0.state.recent_tags, ["from-b"]);
    }

    // spec: NET-016
    #[test]
    fn a_failed_placement_keeps_the_state_in_the_vault_and_says_why() {
        let (root, base) = (tmp("fail-v"), tmp("fail-b"));
        // the base is a file, so no folder can be made in it
        put(&base.join("file"), "x");
        let msg = place(&root, true, &base.join("file")).expect("a message");
        assert!(msg.contains("Could not make"), "{msg}");
        assert_eq!(local_dir(&root), root.join(".shale"));
        // an ID that cannot be written (the vault is a file)
        let not_a_dir = base.join("file");
        let msg = place(&not_a_dir, true, &base).expect("a message");
        assert!(msg.contains("outside the synced folder"), "{msg}");
    }

    // spec: NET-016
    #[test]
    fn the_sweep_removes_stale_temp_files_and_conflict_copies_of_the_local_files_only() {
        let root = tmp("sweep");
        let day = Duration::from_secs(24 * 3600);
        let old_tmp = root.join("sub/.shale-tmp-1-2-3");
        let old_tmp_shale = root.join(".shale/.shale-tmp-9-9");
        let new_tmp = root.join("sub/.shale-tmp-4-5-6");
        let git_tmp = root.join(".git/.shale-tmp-1-1");
        for (p, age) in [(&old_tmp, 2 * day), (&old_tmp_shale, 3 * day), (&new_tmp, Duration::from_secs(60)), (&git_tmp, 5 * day)] {
            put(p, "x");
            set_age(p, age);
        }
        let notes = ["a.md", "sub/b.md", "sub/c.sync-conflict-20260101-000000-ABC.md", "sub/.shale-tmpfile", "sub/shale-tmp-x"];
        for n in notes {
            put(&root.join(n), "note");
            set_age(&root.join(n), 9 * day);
        }
        let gone = [".shale/state.sync-conflict-20260101-000000-ABC.json", ".shale/undo/0123.sync-conflict-20260101-000000-ABC.json"];
        let kept = [".shale/state.json", ".shale/undo/0123.json", ".shale/settings.sync-conflict-20260101-000000-ABC.json", ".shale/tag-words.sync-conflict-20260101-000000-ABC.json", ".shale/state.json.broken-2026"];
        for n in gone.iter().chain(&kept) {
            put(&root.join(n), "x");
        }
        let swept = sweep(&root, SystemTime::now());
        assert_eq!(swept, Swept { temp: 2, conflicts: 2 });
        assert!(!old_tmp.exists() && !old_tmp_shale.exists());
        assert!(new_tmp.exists(), "a young temp file may belong to a running save");
        assert!(git_tmp.exists(), "nothing in .git is touched");
        for n in notes.iter().chain(&kept) {
            assert!(root.join(n).exists(), "{n} must stay");
        }
        for n in gone {
            assert!(!root.join(n).exists(), "{n} must go");
        }
        assert_eq!(sweep(&root, SystemTime::now()), Swept::default());
    }

    // spec: NET-016
    #[test]
    fn the_sweep_also_cleans_the_folder_outside_the_vault() {
        let (root, base) = (tmp("sweep-out-v"), tmp("sweep-out-b"));
        assert_eq!(place(&root, true, &base), None);
        let out = local_dir(&root);
        put(&out.join("state.sync-conflict-20260101-000000-ABC.json"), "x");
        put(&out.join("undo/1.sync-conflict-20260101-000000-ABC.json"), "x");
        put(&out.join("state.json"), "{}");
        assert_eq!(sweep(&root, SystemTime::now()), Swept { temp: 0, conflicts: 2 });
        assert!(out.join("state.json").is_file());
    }
}
