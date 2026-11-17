//! A folder of the vault as a book (BOOK, #306): which notes are chapters, which folder notes
//! introduce which parts, in what order they are read, and what they are called. Pure functions
//! over the [`Vault`]; the PDF is laid out by `book_export`.

use crate::markdown;
use crate::vault::{FileKind, Vault};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A note of a part, or a part inside it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Part(Part),
    Chapter(Chapter),
}

/// A subfolder (BOOK-002): its folder note's text first, then its items.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub dir: PathBuf,
    pub title: String,
    /// The folder note when it is printed (not `export: false`).
    pub note: Option<PathBuf>,
    pub items: Vec<Item>,
}

/// A note (BOOK-001).
#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub rel: PathBuf,
    pub title: String,
    /// `export: false`: not printed; the reading order shows it greyed.
    pub left_out: bool,
}

/// The book of a folder: what is in it, in what order (BOOK-001…BOOK-004).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub dir: PathBuf,
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    /// The top folder's note, printed before the first part or chapter.
    pub intro: Option<PathBuf>,
    /// The top folder's note even when it is not printed: its front matter fills the fields of a
    /// LaTeX template (BOOK-015).
    pub note: Option<PathBuf>,
    pub items: Vec<Item>,
}

/// A line of the reading order as the export dialog shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// 0 for what is directly in the top folder.
    pub depth: usize,
    /// `2`, `2.1`; none for what is left out.
    pub number: Option<String>,
    pub title: String,
    pub part: bool,
    pub left_out: bool,
}

impl Plan {
    /// The reading order as numbered lines: every part and chapter, in order, the notes that are left
    /// out greyed in their place and without a number.
    pub fn lines(&self) -> Vec<Line> {
        fn walk(items: &[Item], depth: usize, prefix: &str, out: &mut Vec<Line>) {
            let mut n = 0;
            for item in items {
                match item {
                    Item::Chapter(c) if c.left_out => out.push(Line { depth, number: None, title: c.title.clone(), part: false, left_out: true }),
                    Item::Chapter(c) => {
                        n += 1;
                        out.push(Line { depth, number: Some(join(prefix, n)), title: c.title.clone(), part: false, left_out: false });
                    }
                    Item::Part(p) => {
                        n += 1;
                        let number = join(prefix, n);
                        out.push(Line { depth, number: Some(number.clone()), title: p.title.clone(), part: true, left_out: false });
                        walk(&p.items, depth + 1, &number, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, 0, "", &mut out);
        out
    }

    /// Every note that is printed, in reading order: the intro, then the parts' folder notes and the chapters.
    pub fn notes(&self) -> Vec<PathBuf> {
        fn walk(items: &[Item], out: &mut Vec<PathBuf>) {
            for item in items {
                match item {
                    Item::Chapter(c) if !c.left_out => out.push(c.rel.clone()),
                    Item::Chapter(_) => {}
                    Item::Part(p) => {
                        out.extend(p.note.clone());
                        walk(&p.items, out);
                    }
                }
            }
        }
        let mut out: Vec<PathBuf> = self.intro.iter().cloned().collect();
        walk(&self.items, &mut out);
        out
    }
}

fn join(prefix: &str, n: usize) -> String {
    if prefix.is_empty() {
        n.to_string()
    } else {
        format!("{prefix}.{n}")
    }
}

/// The first value of a front matter property of a note.
pub fn property(text: &str, key: &str) -> Option<String> {
    markdown::front_matter_values(text).into_iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).and_then(|(_, v)| v.into_iter().find(|s| !s.trim().is_empty())).map(|s| s.trim().to_string())
}

/// Whether the note's front matter says `key: true`.
fn flag(text: &str, key: &str) -> bool {
    property(text, key).is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "yes"))
}

/// `export: false` (BOOK-001).
pub fn left_out(text: &str) -> bool {
    property(text, "export").is_some_and(|v| matches!(v.to_lowercase().as_str(), "false" | "no"))
}

/// Compare names so that `2 Lists` comes before `10 Tables` and case does not matter (BOOK-003).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.extend(c.to_lowercase()),
                _ => out.push((digit, c.to_lowercase().collect())),
            }
        }
        out
    }
    let (x, y) = (chunks(a), chunks(b));
    for ((dx, sx), (dy, sy)) in x.iter().zip(&y) {
        let o = if *dx && *dy {
            // numbers by value, however many digits
            let (tx, ty) = (sx.trim_start_matches('0'), sy.trim_start_matches('0'));
            tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
        } else {
            sx.cmp(sy)
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

/// What is in a folder before it is put in order.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Note(PathBuf),
    Folder(PathBuf),
}

impl Entry {
    fn path(&self) -> &Path {
        match self {
            Entry::Note(p) | Entry::Folder(p) => p,
        }
    }
    /// The name `order:` and the sorting use: a note's without `.md`, a folder's.
    fn name(&self) -> String {
        let name = self.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match self {
            Entry::Note(_) => markdown::strip_md_ext(&name).to_string(),
            Entry::Folder(_) => name,
        }
    }
}

struct Ctx<'a> {
    vault: &'a Vault,
}

impl Ctx<'_> {
    fn text(&self, rel: &Path) -> &str {
        self.vault.notes.get(rel).map(|n| n.text.as_str()).unwrap_or("")
    }

    /// The notes directly in `dir`, as `(rel, file name without .md)`.
    fn notes_in(&self, dir: &Path) -> Vec<PathBuf> {
        self.vault.files.iter().filter(|f| f.kind == FileKind::Note && f.rel.parent() == Some(dir)).map(|f| f.rel.clone()).collect()
    }

    /// The subfolders of `dir` that can be parts (BOOK-001): not `.x`, not `_x`, and at the vault's root not `exports` or `templates`.
    fn folders_in(&self, dir: &Path) -> Vec<PathBuf> {
        self.vault
            .folders
            .iter()
            .filter(|f| f.parent() == Some(dir))
            .filter(|f| {
                let name = f.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                let reserved = dir.as_os_str().is_empty() && (name == "exports" || name == "templates");
                !name.starts_with('.') && !name.starts_with('_') && !reserved
            })
            .cloned()
            .collect()
    }

    /// The note that introduces `dir` (BOOK-002).
    fn folder_note(&self, dir: &Path) -> Option<PathBuf> {
        let mut notes = self.notes_in(dir);
        notes.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let folder = dir.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_else(|| self.vault_name().to_lowercase());
        for want in ["index", "readme"] {
            if let Some(n) = notes.iter().find(|n| stem(n) == want) {
                return Some(n.clone());
            }
        }
        notes.iter().find(|n| stem(n) == folder).or_else(|| notes.iter().find(|n| flag(self.text(n), "folder-note"))).cloned()
    }

    fn vault_name(&self) -> String {
        self.vault.root.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Vault".to_string())
    }

    fn folder_name(&self, dir: &Path) -> String {
        dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.vault_name())
    }

    /// A chapter's title (BOOK-004): `title:`, else the title line.
    fn chapter_title(&self, rel: &Path) -> String {
        property(self.text(rel), "title").unwrap_or_else(|| self.vault.file(rel).map(|f| f.title()).unwrap_or_else(|| crate::vault::title_of(&rel.to_string_lossy()).to_string()))
    }

    /// The entries of `dir` in reading order (BOOK-003); `note` is its folder note, which is not an entry.
    fn ordered(&self, dir: &Path, note: Option<&Path>) -> Vec<Entry> {
        let mut all: Vec<Entry> = self.notes_in(dir).into_iter().filter(|n| Some(n.as_path()) != note).map(Entry::Note).chain(self.folders_in(dir).into_iter().map(Entry::Folder)).collect();
        all.sort_by(|a, b| natural_cmp(&a.name(), &b.name()));
        let mut out: Vec<Entry> = Vec::new();
        let take = |e: &Entry, out: &mut Vec<Entry>| {
            if !out.contains(e) {
                out.push(e.clone());
            }
        };
        if let Some(note) = note {
            let text = self.text(note);
            let named = markdown::front_matter_values(text).into_iter().find(|(k, _)| k.eq_ignore_ascii_case("order")).map(|(_, v)| v).unwrap_or_default();
            let named: Vec<String> = named.iter().map(|n| markdown::strip_md_ext(n.trim().trim_start_matches("[[").trim_end_matches("]]").trim_end_matches('/')).to_lowercase()).filter(|n| !n.is_empty()).collect();
            if !named.is_empty() {
                for name in &named {
                    all.iter().filter(|e| e.name().to_lowercase() == *name).for_each(|e| take(e, &mut out));
                }
            } else {
                let body: String = text.lines().skip(markdown::front_matter_lines(text)).collect::<Vec<_>>().join("\n");
                for link in markdown::find_links(&body).into_iter().filter(|l| !l.embed) {
                    let Some(target) = self.vault.resolve(&link.target, Some(note)) else { continue };
                    if target.as_path() == note {
                        continue;
                    }
                    let dir_of = target.parent().unwrap_or(Path::new(""));
                    // a note of this folder; or the folder note of a subfolder, which stands for the subfolder
                    let hit = all.iter().find(|e| match e {
                        Entry::Note(p) => *p == target,
                        Entry::Folder(f) => f.as_path() == dir_of && self.folder_note(f).as_deref() == Some(target.as_path()),
                    });
                    if let Some(e) = hit {
                        take(e, &mut out);
                    }
                }
            }
        }
        for e in &all {
            take(e, &mut out);
        }
        out
    }

    /// A part: the folder `dir` as an item, None when it has no chapter (BOOK-001).
    fn part(&self, dir: &Path) -> Option<Part> {
        let note = self.folder_note(dir);
        let items = self.items(dir, note.as_deref());
        if !has_chapter(&items) {
            return None;
        }
        let title = note.as_deref().and_then(|n| property(self.text(n), "title")).unwrap_or_else(|| self.folder_name(dir));
        let printed = note.filter(|n| !left_out(self.text(n)));
        Some(Part { dir: dir.to_path_buf(), title, note: printed, items })
    }

    fn items(&self, dir: &Path, note: Option<&Path>) -> Vec<Item> {
        self.ordered(dir, note)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Note(rel) => Some(Item::Chapter(Chapter { title: self.chapter_title(&rel), left_out: left_out(self.text(&rel)), rel })),
                Entry::Folder(f) => self.part(&f).map(Item::Part),
            })
            .collect()
    }
}

fn has_chapter(items: &[Item]) -> bool {
    items.iter().any(|i| match i {
        Item::Chapter(c) => !c.left_out,
        Item::Part(p) => has_chapter(&p.items),
    })
}

/// The book of `dir` (a folder of the vault, `""` for the vault itself): its chapters and parts in
/// reading order and its title page data. An error says why there is no book.
pub fn plan(vault: &Vault, dir: &Path) -> Result<Plan, String> {
    if !dir.as_os_str().is_empty() && !vault.folders.iter().any(|f| f == dir) {
        return Err(format!("There is no folder “{}” in the vault", dir.display()));
    }
    let cx = Ctx { vault };
    let note = cx.folder_note(dir);
    let items = cx.items(dir, note.as_deref());
    let name = cx.folder_name(dir);
    if !has_chapter(&items) {
        return Err(format!("“{name}” has no notes to export"));
    }
    let text = note.as_deref().map(|n| cx.text(n)).unwrap_or("");
    let intro = note.clone().filter(|n| !left_out(cx.text(n)));
    Ok(Plan { dir: dir.to_path_buf(), title: property(text, "title").unwrap_or(name), subtitle: property(text, "subtitle"), author: property(text, "author"), date: property(text, "date"), intro, note, items })
}

/// The notes of the book as a set, for the links between chapters (BOOK-009).
pub fn members(plan: &Plan) -> HashSet<PathBuf> {
    plan.notes().into_iter().collect()
}
