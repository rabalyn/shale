//! Text that could not be saved when the app quit (DATA-011).
//!
//! If a note's buffer still cannot be written to the vault at exit, the text is put into
//! `<data folder>/shale/recovery/<date>/<path of the note in the vault>` instead of being lost.
//! Nothing there is ever overwritten: a second rescue of the same note gets a numbered name.
//! At the next start the person is told, with a way to open the folder.

use crate::fsutil;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Marker file: this day's folder has been announced and dismissed.
const SEEN: &str = ".seen";

/// The recovery state of the app: the folder rescued text goes to, and what the person has not seen yet.
pub struct RecoveryState {
    /// where text that could not be saved at exit is put (DATA-011)
    pub root: PathBuf,
    /// recovery folders from earlier runs the person has not dismissed yet
    pub notice: Vec<PathBuf>,
}

/// The default recovery folder.
pub fn default_root() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("recovery")
}

/// Keep only the normal parts of a vault-relative path, so a rescue can never leave its folder.
fn safe_rel(rel: &Path) -> PathBuf {
    let p: PathBuf = rel.components().filter_map(|c| if let Component::Normal(n) = c { Some(n) } else { None }).collect();
    if p.as_os_str().is_empty() {
        PathBuf::from("note.md")
    } else {
        p
    }
}

/// `Name.md` → `Name (2).md`
fn numbered(p: &Path, n: u32) -> PathBuf {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match p.extension() {
        Some(e) => format!("{stem} ({n}).{}", e.to_string_lossy()),
        None => format!("{stem} ({n})"),
    };
    p.with_file_name(name)
}

/// Write `bytes` for the note `rel` into `root/<date>/<rel>` and return where they went. Never
/// replaces a file that is there already.
pub fn rescue(root: &Path, date: &str, rel: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let _flow = tracing::info_span!("flow.recovery_rescue", peer = "app").entered();
    let target = root.join(date).join(safe_rel(rel));
    if let Some(dir) = target.parent() {
        fsutil::ensure_private_dir(root)?;
        std::fs::create_dir_all(dir)?;
    }
    let mut candidate = target.clone();
    for n in 2..1000 {
        match tracing::info_span!("flow.recovery_rescue.write_copy", peer = "disk").in_scope(|| fsutil::create_new(&candidate, bytes)) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => candidate = numbered(&target, n),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many rescued copies of the same note"))
}

/// The day folders in `root` that have not been dismissed yet, oldest first.
pub fn unseen(root: &Path) -> Vec<PathBuf> {
    let _flow = tracing::info_span!("flow.recovery_unseen", peer = "app").entered();
    let Ok(rd) = tracing::info_span!("flow.recovery_unseen.list_days", peer = "disk").in_scope(|| std::fs::read_dir(root)) else { return vec![] };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.join(SEEN).exists()).collect();
    v.sort();
    v
}

/// The person has seen the notice: it is not shown again for these folders.
pub fn dismiss(dirs: &[PathBuf]) {
    let _flow = tracing::info_span!("flow.recovery_dismiss", peer = "app").entered();
    for d in dirs {
        // a marker that already exists is fine
        let _ = tracing::info_span!("flow.recovery_dismiss.mark_seen", peer = "disk").in_scope(|| fsutil::create_new(&d.join(SEEN), b""));
    }
}

/// Is `p` a folder inside the recovery folder (the only place [`crate::opener`] opens for this)?
pub fn is_inside(root: &Path, p: &Path) -> bool {
    match (std::fs::canonicalize(root), std::fs::canonicalize(p)) {
        (Ok(r), Ok(p)) => p.starts_with(r) && p.is_dir(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-recovery-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    // spec: DATA-011
    #[test]
    fn a_rescued_note_lands_under_its_vault_path_and_is_never_overwritten() {
        let root = tmp("rescue");
        let a = rescue(&root, "2026-11-14", Path::new("Projects/Plan.md"), b"one").unwrap();
        assert_eq!(a, root.join("2026-11-14/Projects/Plan.md"));
        let b = rescue(&root, "2026-11-14", Path::new("Projects/Plan.md"), b"two").unwrap();
        assert_eq!(b, root.join("2026-11-14/Projects/Plan (2).md"));
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        // a path that tries to climb out stays inside
        let c = rescue(&root, "2026-11-14", Path::new("../../evil.md"), b"x").unwrap();
        assert!(c.starts_with(&root), "{c:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: DATA-011
    #[test]
    fn a_notice_is_shown_until_it_is_dismissed() {
        let root = tmp("notice");
        assert!(unseen(&root).is_empty());
        rescue(&root, "2026-11-13", Path::new("A.md"), b"a").unwrap();
        rescue(&root, "2026-11-14", Path::new("B.md"), b"b").unwrap();
        let u = unseen(&root);
        assert_eq!(u.len(), 2);
        assert!(is_inside(&root, &u[0]) && !is_inside(&root, Path::new("/")));
        dismiss(&u);
        dismiss(&u); // twice is fine
        assert!(unseen(&root).is_empty());
        rescue(&root, "2026-11-15", Path::new("C.md"), b"c").unwrap();
        assert_eq!(unseen(&root).len(), 1, "only the new day");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: DEV-031
    #[test]
    fn dataflow_recovery_rescue() {
        let root = tmp("dataflow-rescue");
        let flow = crate::dataflow::record(|| {
            rescue(&root, "2026-11-14", Path::new("Projects/Plan.md"), b"one").unwrap();
        });
        crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/recovery.md"), "recovery_rescue", &crate::dataflow::render_mermaid(&flow));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: DEV-031
    #[test]
    fn dataflow_recovery_unseen() {
        let root = tmp("dataflow-unseen");
        rescue(&root, "2026-11-14", Path::new("Plan.md"), b"one").unwrap();
        let flow = crate::dataflow::record(|| {
            assert_eq!(unseen(&root).len(), 1);
        });
        crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/recovery.md"), "recovery_unseen", &crate::dataflow::render_mermaid(&flow));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: DEV-031
    #[test]
    fn dataflow_recovery_dismiss() {
        let root = tmp("dataflow-dismiss");
        rescue(&root, "2026-11-14", Path::new("Plan.md"), b"one").unwrap();
        let days = unseen(&root);
        let flow = crate::dataflow::record(|| dismiss(&days));
        crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/recovery.md"), "recovery_dismiss", &crate::dataflow::render_mermaid(&flow));
        let _ = std::fs::remove_dir_all(&root);
    }
}
