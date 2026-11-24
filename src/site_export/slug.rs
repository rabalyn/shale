//! Where a note lives on the site (SITE-022, SITE-041): its vault path made into a slug, the same
//! one on every export, and never the same for two paths.

use ring::digest;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Where a page is: its file below `content/` and its URL (root-relative, percent-encoded).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub content: String,
    pub url: String,
}

/// The pages of the site: a place for each published note, and for each folder that holds one (the
/// section), with the folder note that is the section's text (SITE-041).
#[derive(Debug, Default)]
pub struct Layout {
    /// a published note (a folder note has the place of its section)
    pub notes: HashMap<PathBuf, Place>,
    /// a folder with published notes below it; the vault root is the empty path
    pub sections: BTreeMap<PathBuf, Place>,
    /// the folder note of a folder: the note whose text the section shows
    pub folder_notes: HashMap<PathBuf, PathBuf>,
}

/// URL paths the site uses for itself: a note may not take them.
const RESERVED: [&str; 6] = ["tags", "categories", "media", "pagefind", "themes", "shale"];

/// A name made into one URL segment: lower case, letters and digits kept, everything else a `-`,
/// no run of `-`, none at the ends (SITE-022). A name with nothing to keep is `note`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "note".into()
    } else {
        out.into()
    }
}

/// Six hex digits of the SHA-256 of a name: what tells two names with one slug apart.
fn short_hash(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let d = digest::digest(&digest::SHA256, name.nfc().collect::<String>().as_bytes());
    d.as_ref()[..3].iter().map(|b| format!("{b:02x}")).collect()
}

/// `s` as a part of a URL: bytes that are not unreserved are `%XX` (SITE-019).
pub fn encode_url(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The folder note of `dir` (BOOK-002) among its published `notes`: `index`, else `readme`, else
/// the note named like the folder, else the one whose text says `folder-note: true`.
fn folder_note(dir: &Path, vault_name: &str, notes: &[PathBuf], flagged: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut notes = notes.to_vec();
    notes.sort();
    let folder = if dir.as_os_str().is_empty() { vault_name.to_lowercase() } else { name_of(dir).to_lowercase() };
    for want in ["index", "readme"] {
        if let Some(n) = notes.iter().find(|n| stem(n) == want) {
            return Some(n.clone());
        }
    }
    notes.iter().find(|n| stem(n) == folder).or_else(|| notes.iter().find(|n| flagged(n))).cloned()
}

impl Layout {
    /// The layout of the notes `published` (vault paths). `vault_name` names the root for its folder
    /// note; `flagged` says whether a note's front matter has `folder-note: true`.
    pub fn build(published: &[PathBuf], vault_name: &str, flagged: &dyn Fn(&Path) -> bool) -> Layout {
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::from([PathBuf::new()]);
        for n in published {
            let mut d = n.parent();
            while let Some(p) = d.filter(|p| !p.as_os_str().is_empty()) {
                dirs.insert(p.to_path_buf());
                d = p.parent();
            }
        }
        let mut layout = Layout::default();
        // top-down: a folder's place needs its parent's
        let mut order: Vec<&PathBuf> = dirs.iter().collect();
        order.sort_by_key(|d| d.components().count());
        let mut taken: HashMap<PathBuf, (String, String)> = HashMap::from([(PathBuf::new(), (String::new(), "/".into()))]);
        for dir in order {
            let notes: Vec<PathBuf> = published.iter().filter(|n| n.parent().unwrap_or(Path::new("")) == dir).cloned().collect();
            let fnote = folder_note(dir, vault_name, &notes, flagged);
            if let Some(f) = &fnote {
                layout.folder_notes.insert(dir.clone(), f.clone());
            }
            let (content_dir, url_dir) = taken.get(dir).cloned().unwrap_or_default();
            // the folders and notes below `dir`, each with its name and whether it is a folder
            let mut names: Vec<(String, bool)> = dirs.iter().filter(|d| d.parent() == Some(dir.as_path()) && *d != dir).map(|d| (name_of(d), true)).collect();
            names.extend(notes.iter().filter(|n| Some(*n) != fnote.as_ref()).map(|n| (n.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), false)));
            let mut by_slug: HashMap<String, Vec<(String, bool)>> = HashMap::new();
            for (name, folder) in &names {
                by_slug.entry(slug(name)).or_default().push((name.clone(), *folder));
            }
            let mut segment: HashMap<(String, bool), String> = HashMap::new();
            for (s, group) in &by_slug {
                let reserved = dir.as_os_str().is_empty() && RESERVED.contains(&s.as_str());
                let folders = group.iter().filter(|g| g.1).count();
                for (name, folder) in group {
                    // a lone folder keeps the slug against notes; every other clash gets the hash
                    let plain = !reserved && (group.len() == 1 || (folders == 1 && *folder));
                    segment.insert((name.clone(), *folder), if plain { s.clone() } else { format!("{s}-{}", short_hash(name)) });
                }
            }
            let section_place = |content_dir: &str, url_dir: &str| Place { content: if content_dir.is_empty() { "_index.md".into() } else { format!("{content_dir}/_index.md") }, url: url_dir.to_string() };
            layout.sections.insert(dir.clone(), section_place(&content_dir, &url_dir));
            if let Some(f) = &fnote {
                layout.notes.insert(f.clone(), section_place(&content_dir, &url_dir));
            }
            for d in dirs.iter().filter(|d| d.parent() == Some(dir.as_path()) && *d != dir) {
                let seg = &segment[&(name_of(d), true)];
                let content = if content_dir.is_empty() { seg.clone() } else { format!("{content_dir}/{seg}") };
                let url = format!("{url_dir}{}/", encode_url(seg));
                taken.insert(d.clone(), (content, url));
            }
            for n in notes.iter().filter(|n| Some(*n) != fnote.as_ref()) {
                let stem = n.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let seg = &segment[&(stem, false)];
                let content = if content_dir.is_empty() { format!("{seg}.md") } else { format!("{content_dir}/{seg}.md") };
                layout.notes.insert(n.clone(), Place { content, url: format!("{url_dir}{}/", encode_url(seg)) });
            }
        }
        layout
    }
}
