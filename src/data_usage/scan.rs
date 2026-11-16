//! Counting what is saved (USE-002…USE-005): the open vault by kind, its largest files and
//! folders, and the person's data outside the vault. Read-only; runs as a background job.

use super::Places;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How many of the largest files and folders are listed (USE-003).
pub const LARGEST: usize = 10;

/// One line: a kind of file, or a folder outside the vault.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    /// the folder, for the lines outside the vault
    pub path: Option<PathBuf>,
    pub files: u64,
    pub bytes: u64,
    /// what could not be read, if anything (counted as far as possible)
    pub problem: Option<String>,
}

/// What is saved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    /// the vault by kind (empty without a vault)
    pub vault: Vec<Line>,
    /// vault-relative paths and sizes, largest first
    pub largest_files: Vec<(PathBuf, u64)>,
    pub largest_folders: Vec<(PathBuf, u64)>,
    pub outside: Vec<Line>,
}

/// The kinds of the vault, in the order they are listed.
const KINDS: [&str; 9] = ["Notes", "Drawings", "Pictures", "PDFs", "Other attachments", ".shale/ (Shale's vault files)", ".trash/", ".git/", "Other hidden files"];

const PICTURES: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tif", "tiff"];

/// The kind (index into [`KINDS`]) of a vault-relative path.
fn kind(rel: &Path) -> usize {
    let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    match first.as_str() {
        ".shale" => return 5,
        ".trash" => return 6,
        ".git" => return 7,
        _ => {}
    }
    if rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
        return 8;
    }
    match crate::vault::kind_of(rel) {
        crate::vault::FileKind::Note => 0,
        crate::vault::FileKind::Drawing => 1,
        crate::vault::FileKind::Other => {
            let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if PICTURES.contains(&ext.as_str()) {
                2
            } else if ext == "pdf" {
                3
            } else {
                4
            }
        }
    }
}

/// Every file under `root` with its size (symbolic links are not followed); the folders that
/// could not be read are named in the problem text.
fn walk(root: &Path) -> (Vec<(PathBuf, u64)>, Option<String>) {
    let mut out = vec![];
    let mut bad = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                if dir.as_path() != root || e.kind() != std::io::ErrorKind::NotFound {
                    bad.push(format!("{}: {e}", dir.display()));
                }
                continue;
            }
        };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push((e.path(), len));
            }
        }
    }
    let problem = (!bad.is_empty()).then(|| format!("could not read {}", bad.join("; ")));
    (out, problem)
}

/// The biggest files or folders with their sizes in bytes.
type Largest = Vec<(PathBuf, u64)>;

/// Count the vault (USE-002, USE-003).
pub fn vault(root: &Path) -> (Vec<Line>, Largest, Largest) {
    let (files, problem) = walk(root);
    let mut lines: Vec<Line> = KINDS.iter().map(|k| Line { label: (*k).to_string(), ..Default::default() }).collect();
    let mut folders: HashMap<PathBuf, u64> = HashMap::new();
    let mut rels = vec![];
    for (abs, len) in files {
        let Ok(rel) = abs.strip_prefix(root) else { continue };
        let k = kind(rel);
        lines[k].files += 1;
        lines[k].bytes += len;
        let mut p = rel.parent();
        while let Some(d) = p {
            if d.as_os_str().is_empty() {
                break;
            }
            *folders.entry(d.to_path_buf()).or_default() += len;
            p = d.parent();
        }
        rels.push((rel.to_path_buf(), len));
    }
    if let Some(p) = problem {
        lines[0].problem = Some(p);
    }
    lines.retain(|l| l.files > 0 || l.problem.is_some() || KINDS[..5].contains(&l.label.as_str()));
    (lines, largest(rels), largest(folders.into_iter().collect()))
}

fn largest(mut v: Vec<(PathBuf, u64)>) -> Vec<(PathBuf, u64)> {
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(LARGEST);
    v
}

/// Count one folder outside the vault; `mine` picks the files that belong to the line.
fn folder_line(label: &str, dir: &Path, mine: impl Fn(&Path) -> bool) -> Line {
    let (files, problem) = walk(dir);
    let (n, bytes) = files.iter().filter(|(p, _)| p.strip_prefix(dir).is_ok_and(&mine)).fold((0, 0), |(n, b), (_, len)| (n + 1, b + len));
    Line { label: label.to_string(), path: Some(dir.to_path_buf()), files: n, bytes, problem }
}

/// The person's data outside the vault (USE-004).
pub fn outside(p: &Places) -> Vec<Line> {
    let mut lines = vec![];
    let mut accounts: Vec<String> = std::fs::read_dir(&p.mail_root)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).filter_map(|e| e.file_name().to_str().map(String::from)).filter(|n| crate::mail_store::valid_account_id(n)).collect())
        .unwrap_or_default();
    accounts.sort();
    for a in accounts {
        lines.push(folder_line(&format!("Mail of {a}"), &p.mail_root.join(&a), |rel| rel.extension().is_some_and(|e| e == "eml")));
    }
    lines.push(folder_line("Mail drafts", &p.mail_root.join("drafts"), |_| true));
    lines.push(folder_line("Mail outbox", &p.mail_root.join("outbox"), |_| true));
    lines.push(folder_line("Mail trash", &p.mail_root.join(".trash"), |_| true));
    lines.push(folder_line("Local calendars", &p.calendars, |rel| !rel.starts_with(".calendar-trash")));
    lines.push(folder_line("Calendar trash", &p.calendar_trash, |_| true));
    lines.push(folder_line("Recovered notes", &p.recovery, |_| true));
    lines
}

/// Everything "Saved" shows (a background job).
pub fn saved(p: &Places) -> Saved {
    let (vault, largest_files, largest_folders) = match &p.vault {
        Some(v) => vault(v),
        None => (vec![], vec![], vec![]),
    };
    Saved { vault, largest_files, largest_folders, outside: outside(p) }
}
