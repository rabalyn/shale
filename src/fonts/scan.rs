//! The scan of the fonts of the exports (JOB-040, ADR-0040 §8): the faces of the vault's `fonts/`
//! (and of the template sets' own `fonts/`) and of the system, each with its family names, style,
//! weight, file and licence. It runs in a quiet background job of the busy registry and is cached
//! for the session; the vault's part is read again when its font files change.

use super::licence::{self, Licence};
use crate::busy::{Busy, Job, Key, LISTED_AFTER};
use crate::vault::Vault;
use skrifa::FontRef;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;

/// The task of the scan ("Looking up fonts...").
pub const FONT_SCAN: Key = Key::new("fonts-scan");

/// Where a face is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The vault's `fonts/` folder.
    Vault,
    /// The `fonts/` folder of a LaTeX template set (by the set's folder name).
    Set(String),
    /// An installed font.
    System,
}

/// One face of a font file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    /// The family names of the face as the font states them (the typographic family, else the
    /// family; the English name first).
    pub families: Vec<String>,
    pub weight: u16,
    pub italic: bool,
    /// The width is the normal one (not condensed or expanded).
    pub normal_width: bool,
    pub path: PathBuf,
    /// The index of the face in a collection (0 for a single font).
    pub index: u32,
    pub licence: Licence,
    pub place: Place,
}

/// The name of a family as it is compared: case-insensitive, without spaces, `-` and `_` (PDF-035).
pub fn name_key(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '_').flat_map(char::to_lowercase).collect()
}

/// Whether `path` has the extension of a font file this feature reads.
pub fn is_font_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["ttf", "otf", "ttc"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The faces of a database.
fn faces_of(db: &fontdb::Database, place: &Place) -> Vec<Face> {
    db.faces()
        .filter_map(|info| {
            let path = match &info.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => p.clone(),
                fontdb::Source::Binary(_) => return None,
            };
            let licence = db.with_face_data(info.id, |data, index| FontRef::from_index(data, index).map(|f| licence::of_font(&f)).unwrap_or(Licence::Free)).unwrap_or(Licence::Free);
            Some(Face {
                families: info.families.iter().map(|(n, _)| n.clone()).collect(),
                weight: info.weight.0,
                italic: info.style != fontdb::Style::Normal,
                normal_width: info.stretch == fontdb::Stretch::Normal,
                path,
                index: info.index,
                licence,
                place: place.clone(),
            })
        })
        .collect()
}

/// The faces of one font file; none when it is no font.
pub fn faces_of_file(path: &Path, place: &Place) -> Vec<Face> {
    let mut db = fontdb::Database::new();
    if db.load_font_file(path).is_err() {
        return Vec::new();
    }
    faces_of(&db, place)
}

/// The font files below `dir`, in path order; a folder that starts with a dot is not searched.
fn font_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            if depth < 12 {
                font_files(&path, depth + 1, out);
            }
        } else if is_font_file(&path) {
            out.push(path);
        }
    }
}

/// The folders of a vault that hold fonts: its `fonts/`, then `templates/tex/<set>/fonts/` of each set.
fn font_dirs(root: &Path) -> Vec<(PathBuf, Place)> {
    let mut dirs = vec![(root.join("fonts"), Place::Vault)];
    let sets = root.join("templates").join("tex");
    let mut names: Vec<String> = std::fs::read_dir(&sets).into_iter().flatten().filter_map(Result::ok).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    dirs.extend(names.into_iter().map(|n| (sets.join(&n).join("fonts"), Place::Set(n))));
    dirs
}

/// The faces of the font files in the vault's `fonts/` and the sets' `fonts/`.
pub fn scan_vault(root: &Path) -> Vec<Face> {
    let mut faces = Vec::new();
    for (dir, place) in font_dirs(root) {
        let mut files = Vec::new();
        font_files(&dir, 0, &mut files);
        let mut db = fontdb::Database::new();
        for f in files {
            let _ = db.load_font_file(&f);
        }
        faces.extend(faces_of(&db, &place));
    }
    faces
}

/// The faces of the installed fonts, or of the fonts in `dirs` (a test points the scan at a folder).
pub fn scan_system(dirs: Option<&[PathBuf]>) -> Vec<Face> {
    let mut db = fontdb::Database::new();
    match dirs {
        Some(dirs) => dirs.iter().for_each(|d| db.load_fonts_dir(d)),
        None => db.load_system_fonts(),
    }
    faces_of(&db, &Place::System)
}

/// A number that changes when the font files of the vault change: the names and times of the
/// files below `fonts/` and `templates/tex/<set>/fonts/` in the vault's index.
pub fn signature(vault: &Vault) -> u64 {
    let mut h = DefaultHasher::new();
    for f in vault.files.iter().filter(|f| is_font_file(&f.rel)) {
        let parts: Vec<_> = f.rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
        let in_fonts = parts.first().is_some_and(|p| p == "fonts") || (parts.len() > 4 && parts[0] == "templates" && parts[1] == "tex" && parts[3] == "fonts");
        if in_fonts {
            (&f.rel, f.modified).hash(&mut h);
        }
    }
    h.finish()
}

/// What the resolver looks fonts up in: the faces found so far.
#[derive(Clone, Debug, Default)]
pub struct Catalogue {
    /// The faces of the vault's `fonts/` and the sets' `fonts/`.
    pub vault: Arc<Vec<Face>>,
    pub system: Arc<Vec<Face>>,
    /// The system scan is in; until then a name that the vault does not have is not known to be missing.
    pub system_ready: bool,
    /// Changes whenever a scan result is taken in.
    pub generation: u64,
}

struct Scanned {
    system: Option<Vec<Face>>,
    vault: Option<(PathBuf, u64, Vec<Face>)>,
}

/// The scan of a window: asked for by whoever needs the fonts, run in the background, cached.
#[derive(Default)]
pub struct FontScan {
    wanted: bool,
    /// a test reads these folders instead of the installed fonts
    system_dirs: Option<Vec<PathBuf>>,
    catalogue: Catalogue,
    vault_key: Option<(PathBuf, u64)>,
    /// (the vault's `uid`, its `generation`, [`signature`]) of the last look at the index
    signed: Option<(u64, u64, u64)>,
    job: Option<Job<Scanned>>,
}

impl FontScan {
    /// Read the system fonts from these folders instead (a test).
    #[cfg(test)]
    pub fn with_system_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.system_dirs = Some(dirs);
        self
    }

    /// The fonts are needed (the export dialog opens): the next [`FontScan::tick`] starts the scan.
    pub fn request(&mut self) {
        self.wanted = true;
    }

    /// Whether a scan job runs.
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    pub fn catalogue(&self) -> Catalogue {
        self.catalogue.clone()
    }

    /// One step: take in a finished scan, and start one when the fonts are wanted and the system
    /// was never read or the vault's font files differ from the ones last read. `notify`
    /// repaints the window when the job ends.
    pub fn tick(&mut self, vault: Option<&Vault>, busy: &Busy, notify: impl FnOnce() + Send + 'static) {
        if let Some(job) = &self.job {
            let done = match job.try_recv() {
                Ok(Ok(done)) => Some(done),
                // a scan that panicked leaves the faces empty, so the export goes on without them
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => Some(Scanned { system: Some(Vec::new()), vault: None }),
                Err(TryRecvError::Empty) => None,
            };
            let Some(done) = done else { return };
            self.job = None;
            if let Some(system) = done.system {
                self.catalogue.system = Arc::new(system);
                self.catalogue.system_ready = true;
            }
            if let Some((root, sig, faces)) = done.vault {
                self.catalogue.vault = Arc::new(faces);
                self.vault_key = Some((root, sig));
            }
            self.catalogue.generation += 1;
        }
        if !self.wanted {
            return;
        }
        let vault = vault.map(|v| {
            let sig = match self.signed {
                Some((uid, generation, sig)) if uid == v.uid && generation == v.generation => sig,
                _ => {
                    let sig = signature(v);
                    self.signed = Some((v.uid, v.generation, sig));
                    sig
                }
            };
            (v.root.clone(), sig)
        });
        let need_system = !self.catalogue.system_ready;
        let need_vault = vault.is_some() && vault != self.vault_key;
        if vault.is_none() && self.vault_key.is_some() {
            // another vault closed: its fonts are not this one's
            self.catalogue.vault = Arc::default();
            self.vault_key = None;
            self.catalogue.generation += 1;
        }
        if !need_system && !need_vault {
            return;
        }
        let Some(task) = busy.begin_listed_after(FONT_SCAN, "Looking up fonts...", LISTED_AFTER) else { return };
        let dirs = self.system_dirs.clone();
        // a test never reads the machine's fonts (a scan takes seconds and every test that opens an
        // export dialog would start one, in parallel): it reads the folders it names, or none
        #[cfg(test)]
        let dirs = Some(dirs.unwrap_or_default());
        self.job = Some(task.run(
            move || Scanned {
                system: need_system.then(|| scan_system(dirs.as_deref())),
                vault: vault.filter(|_| need_vault).map(|(root, sig)| {
                    let faces = scan_vault(&root);
                    (root, sig, faces)
                }),
            },
            notify,
        ));
    }
}
