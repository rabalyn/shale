//! Caches that are plain files: list them, count them, delete them, prune them by age and size
//! (USE-007, USE-022). The owner of a cache decides which files are its own (exact names or
//! patterns); these helpers never delete a folder.

use super::{CacheStats, Freed};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// One file of a cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub bytes: u64,
    /// when the entry was made, in seconds since 1970 (modification time or a date in the name)
    pub at: i64,
}

/// The files directly in `dir` whose name `mine` accepts; their time is the modification time.
pub fn entries(dir: &Path, mine: impl Fn(&str) -> bool) -> Vec<FileEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<FileEntry> = rd
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(&mine))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            let at = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            Some(FileEntry { path: e.path(), bytes: m.len(), at })
        })
        .collect();
    v.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.path.cmp(&b.path)));
    v
}

/// Size, count and the oldest entry.
pub fn stats(files: &[FileEntry]) -> CacheStats {
    CacheStats { bytes: files.iter().map(|f| f.bytes).sum(), entries: files.len() as u64, oldest: files.iter().map(|f| f.at).min() }
}

/// Delete these files. A file that is already gone counts as deleted; the first other error is
/// returned after trying every file.
pub fn remove(files: &[FileEntry]) -> Result<Freed, String> {
    let mut freed = Freed::default();
    let mut first_error = None;
    for f in files {
        match std::fs::remove_file(&f.path) {
            Ok(()) => {
                freed.bytes += f.bytes;
                freed.entries += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                first_error.get_or_insert_with(|| format!("{}: {e}", f.path.display()));
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(freed),
    }
}

/// What a prune removes: the entries older than `max_age_days`, then the oldest until the rest
/// is at most `max_bytes` and at most `max_count` entries. `None` is no limit.
pub fn over_limits(files: &[FileEntry], now: i64, max_age_days: Option<u32>, max_bytes: Option<u64>, max_count: Option<u64>) -> Vec<FileEntry> {
    let mut sorted = files.to_vec();
    sorted.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.path.cmp(&b.path)));
    let cutoff = max_age_days.map(|d| now - i64::from(d) * 86_400);
    let (mut out, mut rest): (Vec<FileEntry>, Vec<FileEntry>) = sorted.into_iter().partition(|f| cutoff.is_some_and(|c| f.at < c));
    let mut total: u64 = rest.iter().map(|f| f.bytes).sum();
    while let Some(first) = rest.first() {
        let too_big = max_bytes.is_some_and(|m| total > m);
        let too_many = max_count.is_some_and(|m| rest.len() as u64 > m);
        if !too_big && !too_many {
            break;
        }
        total -= first.bytes;
        out.push(rest.remove(0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, bytes: u64, at: i64) -> FileEntry {
        FileEntry { path: PathBuf::from(name), bytes, at }
    }

    // spec: USE-022
    #[test]
    fn a_prune_takes_the_too_old_first_then_the_oldest_until_within_the_limits() {
        let day = 86_400;
        let now = 100 * day;
        let files = vec![e("c", 10, now - day), e("a", 10, now - 40 * day), e("b", 10, now - 20 * day), e("d", 10, now)];
        let names = |v: Vec<FileEntry>| v.into_iter().map(|f| f.path.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(over_limits(&files, now, Some(30), None, None)), ["a"]);
        assert_eq!(names(over_limits(&files, now, Some(30), Some(20), None)), ["a", "b"]);
        assert_eq!(names(over_limits(&files, now, None, None, Some(1))), ["a", "b", "c"]);
        assert!(over_limits(&files, now, None, None, None).is_empty(), "no limit removes nothing");
        assert_eq!(over_limits(&files, now, Some(0), None, None).len(), 3, "0 days: everything before now");
    }

    // spec: USE-007
    #[test]
    fn only_the_files_a_cache_calls_its_own_are_listed_and_removed() {
        let d = std::env::temp_dir().join(format!("shale-usefiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub.png")).unwrap();
        std::fs::write(d.join("a.png"), b"12345").unwrap();
        std::fs::write(d.join("note.md"), b"keep").unwrap();
        let mine = entries(&d, |n| n.ends_with(".png"));
        assert_eq!(mine.len(), 1, "a folder with a matching name is not a cache file");
        assert_eq!(stats(&mine), CacheStats { bytes: 5, entries: 1, oldest: Some(mine[0].at) });
        assert_eq!(remove(&mine), Ok(Freed { bytes: 5, entries: 1 }));
        assert_eq!(remove(&mine), Ok(Freed::default()), "a file already gone is fine");
        assert!(d.join("note.md").exists() && d.join("sub.png").is_dir());
        assert!(entries(&d.join("missing"), |_| true).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
