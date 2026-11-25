//! The PDF of a book (BOOK-005…BOOK-009, #306): a title page, the contents, the notes of the plan
//! of `book` one after the other in the layout of a single note's export (`export::Writer`), with
//! numbered headings, running headers, links between the chapters and nested bookmarks.

use crate::board::resolve::Resolved;
use crate::busy::Cancel;
use crate::fonts::resolve::Values;
use crate::markdown;
use crate::pdf::book::{self, Item, Plan};
use crate::pdf::export::{book_scale, finish_from, fit_text, pdf_image, wrap_pieces, Entry, ExportOptions, Piece, Writer};
use crate::pdf::export_fonts::Made;
use crate::pdf::write::{styled_width, text_width, Bookmark, Dest, Rgba, TextStyle};
use crate::vault::{FileKind, Vault};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const BLACK: Rgba = Rgba(0, 0, 0, 255);
const GREY: Rgba = Rgba(120, 120, 120, 255);
const NUMBER: Rgba = Rgba(130, 130, 130, 255);

/// How high the logo of the title page is, in points (about 3.5 cm, BOOK-019), and the space under it.
const LOGO_HEIGHT: f32 = 100.0;
const LOGO_GAP: f32 = 28.0;

/// The picture of the title page (BOOK-019): what the top folder note's `logo:` names, read as a picture.
/// `Ok(None)` without the property; `Err` says why the picture is skipped.
pub(super) fn logo_picture(vault: &Vault, plan: &Plan) -> Result<Option<image::RgbaImage>, String> {
    let Some(target) = &plan.logo else { return Ok(None) };
    let rel = match vault.resolve_media(target, plan.note.as_deref()) {
        crate::vault::Shown::Vault(rel) => rel,
        _ => return Err(format!("the logo “{target}” is not a file of the vault")),
    };
    let img = image::open(vault.abs(&rel)).map_err(|e| format!("the logo “{target}” cannot be read as a picture: {e}"))?;
    Ok(Some(img.to_rgba8()))
}

/// How far a book export is, and whether it is to stop (JOB-001): shared with the window.
#[derive(Default)]
pub struct Progress {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub cancel: Cancel,
}

impl Progress {
    /// Progress that stops when `cancel` (the token of the export's busy task) is set.
    pub fn with_cancel(cancel: Cancel) -> Self {
        Progress { cancel, ..Default::default() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// the top folder's note: printed before the first part or chapter, without a heading
    Intro,
    Part,
    Chapter,
}

/// A heading of a note with what it is printed as.
struct Head {
    line: usize,
    level: usize,
    /// the title without markup, with its number when it has one
    label: String,
    plain: String,
    /// a heading of the first level inside a chapter is in the contents (BOOK-007)
    listed: bool,
}

/// One part, chapter or the intro with the text it prints.
struct Section {
    kind: Kind,
    rel: Option<PathBuf>,
    level: usize,
    number: Option<String>,
    title: String,
    /// the note's text (the first heading that says the title blanked, BOOK-006)
    text: String,
    numbers: HashMap<usize, String>,
    levels: HashMap<usize, usize>,
    heads: Vec<Head>,
}

impl Section {
    /// `2.1 Lists`: how the section is called in the contents, the bookmarks and the running header.
    fn label(&self) -> String {
        match &self.number {
            Some(n) => format!("{n} {}", self.title),
            None => self.title.clone(),
        }
    }
}

/// The sections of a plan in reading order, with the numbers of the headings worked out (BOOK-006).
fn sections(vault: &Vault, plan: &Plan, numbered: bool) -> Vec<Section> {
    // (a drawing is no note: a board's drawing card is printed as the picture it embeds, BOARD-030)
    let text_of = |rel: &Path| match vault.notes.get(rel) {
        Some(n) => crate::carousel::flatten(&n.text).into_owned(), // a carousel shows every image (PDF-040)
        None if vault.file(rel).is_some_and(|f| f.kind == FileKind::Drawing) => format!("![[{}]]\n", rel.to_string_lossy()),
        None => String::new(),
    };
    let mut out = Vec::new();
    if let Some(rel) = &plan.intro {
        out.push(section(Kind::Intro, Some(rel.clone()), 1, None, plan.title.clone(), text_of(rel)));
    }
    fn walk(items: &[Item], level: usize, prefix: &str, numbered: bool, text_of: &dyn Fn(&Path) -> String, out: &mut Vec<Section>) {
        let mut n = 0;
        for item in items {
            let (kind, rel, title) = match item {
                Item::Chapter(c) if c.left_out => continue,
                Item::Chapter(c) => (Kind::Chapter, Some(c.rel.clone()), c.title.clone()),
                Item::Part(p) => (Kind::Part, p.note.clone(), p.title.clone()),
            };
            n += 1;
            let number = if prefix.is_empty() { n.to_string() } else { format!("{prefix}.{n}") };
            let text = rel.as_deref().map(text_of).unwrap_or_default();
            let s = section(kind, rel, level, numbered.then(|| number.clone()), title, text);
            out.push(s);
            if let Item::Part(p) = item {
                walk(&p.items, level + 1, &number, numbered, text_of, out);
            }
        }
    }
    walk(&plan.items, 1, "", numbered, &text_of, &mut out);
    out
}

/// A section: its headings by depth below the section (BOOK-006).
fn section(kind: Kind, rel: Option<PathBuf>, level: usize, number: Option<String>, title: String, text: String) -> Section {
    // a first `# Heading` that says the title is not printed again: the line is blank (PDF-008)
    let mut text = text;
    if let Some((line, 1, first, _)) = markdown::heading_numbers(&text).into_iter().next() {
        if kind != Kind::Intro && markdown::heading_plain(&first).to_lowercase() == markdown::heading_plain(&title).to_lowercase() {
            let lines: Vec<&str> = text.split('\n').collect();
            text = lines.iter().enumerate().map(|(i, l)| if i == line { "" } else { l }).collect::<Vec<_>>().join("\n");
        }
    }
    let mut numbers = HashMap::new();
    let mut levels = HashMap::new();
    let mut heads = Vec::new();
    for (line, _, raw, label) in markdown::heading_numbers(&text) {
        let own = label.unwrap_or_default();
        let depth = own.split('.').count();
        let printed = match (&number, kind) {
            (Some(n), Kind::Chapter) => Some(format!("{n}.{own}")),
            _ => None,
        };
        let plain = markdown::heading_plain(&raw);
        let label = printed.as_ref().map_or(plain.clone(), |n| format!("{n} {plain}"));
        if let Some(n) = printed {
            numbers.insert(line, n);
        }
        levels.insert(line, level + depth);
        heads.push(Head { line, level: level + depth, label, plain, listed: kind == Kind::Chapter && depth == 1 });
    }
    Section { kind, rel, level, number, title, text, numbers, levels, heads }
}

impl Writer<'_> {
    /// The title page (BOOK-005): the title large, a short rule, the subtitle, and the author and date near the foot.
    fn title_page(&mut self, plan: &Plan, logo: Option<&image::RgbaImage>) {
        let (w, h) = (self.w, self.h);
        let cw = self.cw() * 0.85;
        let centred = |page: &mut crate::pdf::write::Page, style: TextStyle, y: f32, s: &str| {
            let x = (w - styled_width(&style, s)) / 2.0;
            page.text(style, x, y, s);
        };
        let mut y = h * 0.32;
        let size = 34.0;
        if let Some(img) = logo {
            // centred above the title, LOGO_HEIGHT high (as wide as the picture's shape says, at most half the text width)
            let scale = (LOGO_HEIGHT / img.height() as f32).min(self.cw() * 0.5 / img.width() as f32);
            let (lw, lh) = (img.width() as f32 * scale, img.height() as f32 * scale);
            let top = (y - size - LOGO_GAP - lh).max(self.top);
            let idx = self.doc.add_image(pdf_image(img));
            self.p().image(idx, (w - lw) / 2.0, top, lw, lh);
        }
        // the front of a book is set in the book's one font (BOOK-022)
        let font = self.opts.headings();
        let title = TextStyle { bold: true, ..TextStyle::plain(font, size, BLACK) };
        for line in wrap_pieces(&[Piece::plain(&plan.title, size, BLACK, true).in_font(font)], cw) {
            let text: String = line.iter().map(|p| p.text.as_str()).collect();
            centred(self.p(), title, y, text.trim_end());
            y += size * 1.25;
        }
        y += size * 0.35;
        self.p().rect((w - 56.0) / 2.0, y, 56.0, 2.5, Some(Rgba(105, 65, 198, 255)), None);
        y += 2.5 + size * 0.9;
        if let Some(sub) = &plan.subtitle {
            let style = TextStyle::plain(font, 16.0, Rgba(85, 85, 85, 255));
            for line in wrap_pieces(&[Piece::plain(sub, 16.0, BLACK, false).in_font(font)], cw) {
                let text: String = line.iter().map(|p| p.text.as_str()).collect();
                centred(self.p(), style, y, text.trim_end());
                y += 16.0 * 1.5;
            }
        }
        let mut y = h * 0.8;
        for (text, size) in [(plan.author.as_deref(), 13.0), (plan.date.as_deref(), 11.0)] {
            if let Some(text) = text {
                centred(self.p(), TextStyle::plain(font, size, GREY), y, text);
                y += size * 1.6;
            }
        }
    }

    /// Where a part or chapter starts (BOOK-005): on a new page, or two lines further down. The
    /// pages it takes from now on say `header` at the top (BOOK-008).
    fn begin(&mut self, header: String) {
        self.running = Some(header);
        let fresh = self.opts.new_page_each;
        if fresh && self.y > self.top + 0.5 {
            self.new_page();
        } else if !fresh {
            self.y += self.opts.font_pt * 2.0;
        }
        if fresh {
            let (page, title) = (self.page, self.running.clone());
            self.heads[page].title = title;
            self.heads[page].first = true;
        }
    }

    /// The title of a part or chapter (BOOK-006): big, its number in grey before it. Returns where it is.
    fn title(&mut self, level: usize, number: Option<&str>, title: &str) -> Dest {
        let base = self.opts.font_pt;
        let size = base * book_scale(level);
        let at_top = self.y <= self.top + 0.5;
        self.y += if at_top { base * [0.0, 3.5, 2.5, 1.5, 1.0, 1.0][level.min(5)] } else { base * 1.2 };
        let font = self.opts.headings();
        let mut ps = vec![Piece::plain(title, size, BLACK, true).in_font(font)];
        if let Some(n) = number {
            ps.insert(0, Piece::plain(&format!("{n}  "), size, NUMBER, false).in_font(font));
        }
        self.need(size * 3.0);
        let dest = Dest { page: self.page, y: self.y };
        let (x, cw) = (self.left, self.cw());
        self.paragraph(&ps, x, cw);
        if level <= 2 {
            let y = self.y + size * 0.05;
            self.p().line([x, y], [x + cw, y], Rgba(200, 200, 205, 255), 0.7);
        }
        self.y += base * 0.9;
        dest
    }

    /// The small grey header at the top right of a page of a chapter, with a thin line under it (BOOK-008).
    fn running_headers(&mut self) {
        let (left, right, size) = (self.left, self.right, 8.5);
        for i in 0..self.doc.pages.len() {
            let Some(title) = self.heads[i].title.clone().filter(|_| !self.heads[i].first) else { continue };
            let width = self.doc.pages[i].width;
            let room = width - left - right;
            let font = self.opts.headings();
            let text = fit_text(font, &title, size, room);
            let base = self.top * 0.55;
            let page = self.doc.page(i);
            page.text(TextStyle::plain(font, size, GREY), width - right - text_width(font, size, &text), base, &text);
            page.line([left, base + 4.0], [width - right, base + 4.0], Rgba(205, 205, 210, 255), 0.5);
        }
    }
}

/// The PDF of the book of `dir` (BOOK-005…BOOK-009). `progress` counts the notes laid out and can stop it.
pub fn export_book(vault: &Vault, dir: &Path, opts: &ExportOptions, progress: Option<&Progress>) -> Result<Vec<u8>, String> {
    export_book_made(vault, dir, opts, progress).map(|m| m.bytes)
}

/// [`export_book`] with the warnings about fonts (PDF-037, BOOK-022).
pub fn export_book_made(vault: &Vault, dir: &Path, opts: &ExportOptions, progress: Option<&Progress>) -> Result<Made, String> {
    let _flow = tracing::info_span!("flow.book_export", peer = "export").entered();
    let plan = tracing::info_span!("flow.book_export.plan", peer = "vault").in_scope(|| book::plan(vault, dir))?;
    lay_out(vault, &plan, opts, progress, None)
}

/// The PDF of `plan`, the book of a folder or of a board (BOARD-030): with a `board`, its landscape
/// pages come after the title page and the contents and before the first note.
pub(super) fn lay_out(vault: &Vault, plan: &Plan, opts: &ExportOptions, progress: Option<&Progress>, board: Option<&Resolved>) -> Result<Made, String> {
    // one font for the whole book: the top folder note's, else the vault's; a chapter's own is ignored (BOOK-022)
    let note_of = |rel: &Path| vault.notes.get(rel).map(|n| n.text.as_str()).unwrap_or_default();
    let top = plan.note.as_deref().or(plan.intro.as_deref());
    let chapters: Vec<(String, Values)> = plan.notes().iter().filter(|rel| Some(rel.as_path()) != top).map(|rel| (rel.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), Values::of_note(note_of(rel)))).collect();
    let top = top.map(|rel| Values::of_note(note_of(rel))).unwrap_or_default();
    let (opts, warnings) = crate::pdf::export_fonts::apply(opts, &vault.root, top, Some(chapters));
    let opts = &opts;
    let secs = tracing::info_span!("flow.book_export.read_notes", peer = "vault").in_scope(|| sections(vault, plan, opts.numbered_headings));
    if let Some(p) = progress {
        p.total.store(secs.len(), Ordering::SeqCst);
    }
    let mut w = Writer::new(&plan.title, opts);
    w.members = Some(book::members(plan));
    let base = opts.font_pt;
    if opts.title {
        let logo = logo_picture(vault, plan).unwrap_or_else(|why| {
            tracing::warn!("book title page: {why}; the title page has no picture");
            None
        });
        w.title_page(plan, logo.as_ref());
        w.new_page();
    }
    // the contents: parts, chapters and the first level of headings inside a chapter (BOOK-007)
    let mut listed: Vec<(usize, usize)> = Vec::new(); // (section, head index or MAX for the section itself)
    for (i, s) in secs.iter().enumerate() {
        if s.kind == Kind::Intro {
            continue;
        }
        listed.push((i, usize::MAX));
        listed.extend(secs[i].heads.iter().enumerate().filter(|(_, h)| h.listed).map(|(n, _)| (i, n)));
    }
    // a board is the first line of the contents
    let boarded = board.is_some();
    let slots = if opts.toc && !listed.is_empty() {
        let mut gaps: Vec<f32> = listed.iter().enumerate().map(|(n, (i, h))| if *h == usize::MAX && secs[*i].kind == Kind::Part && (n > 0 || boarded) { base * 0.9 } else { 0.0 }).collect();
        if boarded {
            gaps.insert(0, 0.0);
        }
        w.reserve_contents_gapped(&gaps)
    } else {
        vec![]
    };
    // the landscape pages of the board, then the text on portrait pages
    let drawn = board.map(|b| crate::pdf::board::draw_pages(&mut w, vault, b));
    if drawn.is_some() && !secs.is_empty() {
        w.new_page();
    }
    let mut starts: Vec<Option<Dest>> = vec![None; secs.len()];
    let mut places: Vec<HashMap<usize, Dest>> = Vec::new();
    for (i, s) in secs.iter().enumerate() {
        if let Some(p) = progress {
            if p.cancel.is_cancelled() {
                return Err(crate::pdf::export::CANCELLED.into());
            }
            p.done.store(i, Ordering::SeqCst);
        }
        let _chapter = tracing::info_span!("flow.book_export.layout_chapter", peer = "vault").entered();
        if s.kind != Kind::Intro {
            w.begin(s.label());
            starts[i] = Some(w.title(s.level, s.number.as_deref(), &s.title));
        } else {
            starts[i] = Some(Dest { page: w.page, y: w.y });
        }
        let here = match &s.rel {
            Some(rel) => w.note_body(vault, rel, &s.text, &s.numbers, Some(&s.levels)),
            None => HashMap::new(),
        };
        places.push(here);
    }
    // the contents list, now that the pages are known
    let board_entry = drawn.as_ref().map(|d| Entry { level: 1, label: "Board".to_string(), dest: Some(Dest { page: d.first, y: w.top }), bold: false });
    let entries: Vec<Entry> = board_entry
        .into_iter()
        .chain(listed.iter().map(|(i, h)| match h {
            h if *h == usize::MAX => Entry { level: secs[*i].level, label: secs[*i].label(), dest: starts[*i], bold: secs[*i].kind == Kind::Part },
            h => {
                let head = &secs[*i].heads[*h];
                Entry { level: head.level, label: head.label.clone(), dest: places[*i].get(&head.line).copied(), bold: false }
            }
        }))
        .collect();
    w.draw_contents(&entries, &slots);
    // the bookmarks: the board, every part, chapter and heading, nested by level
    if let Some(d) = &drawn {
        w.doc.bookmarks.push(Bookmark { level: 1, title: "Board".to_string(), to: Dest { page: d.first, y: w.top } });
    }
    for (i, s) in secs.iter().enumerate() {
        if s.kind != Kind::Intro {
            if let Some(to) = starts[i] {
                w.doc.bookmarks.push(Bookmark { level: s.level, title: s.label(), to });
            }
        }
        for h in &s.heads {
            if let Some(to) = places[i].get(&h.line) {
                w.doc.bookmarks.push(Bookmark { level: h.level, title: h.label.clone(), to: *to });
            }
        }
    }
    // the links between the chapters (BOOK-009): to the heading, else to where the note starts
    let mut by_note: HashMap<&Path, (Dest, HashMap<String, Dest>)> = HashMap::new();
    for (i, s) in secs.iter().enumerate() {
        if let (Some(rel), Some(start)) = (&s.rel, starts[i]) {
            let heads = s.heads.iter().filter_map(|h| places[i].get(&h.line).map(|d| (h.plain.to_lowercase(), *d))).fold(HashMap::new(), |mut m, (k, d)| {
                m.entry(k).or_insert(d);
                m
            });
            by_note.insert(rel.as_path(), (start, heads));
        }
    }
    // the cards of the board are links to their notes
    for (page, rect, rel) in drawn.iter().flat_map(|d| &d.cards) {
        if let Some((start, _)) = by_note.get(rel.as_path()) {
            w.doc.page(*page).jump(rect[0], rect[1], rect[2], rect[3], *start);
        }
    }
    let links = std::mem::take(&mut w.book_links);
    for l in links {
        let Some((start, heads)) = by_note.get(l.rel.as_path()) else { continue };
        let to = l.heading.as_ref().and_then(|h| heads.get(&markdown::heading_plain(h).to_lowercase())).unwrap_or(start);
        w.doc.page(l.page).jump(l.rect[0], l.rect[1], l.rect[2], l.rect[3], *to);
    }
    w.running_headers();
    if let Some(p) = progress {
        p.done.store(secs.len(), Ordering::SeqCst);
    }
    tracing::info_span!("flow.book_export.write_pdf", peer = "pdf").in_scope(|| finish_from(w.doc, opts, usize::from(opts.title), warnings))
}

/// `shale --export-pdf <folder> <file.pdf>` (BOOK-011): the folder, opened as a vault, as one PDF with the
/// default options (A4, numbered headings, title page, contents, each note on a new page).
pub fn export_folder(folder: &Path, file: &Path) -> Result<(), String> {
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let vault = Vault::open(folder).map_err(|e| format!("could not open {}: {e}", folder.display()))?;
    let bytes = export_book(&vault, Path::new(""), &ExportOptions::for_note(), None)?;
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    crate::fsutil::write_atomic(file, &bytes).map_err(|e| format!("could not write {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
        let dir = std::env::temp_dir().join(format!("shale-book-sec-{tag}-{}", std::process::id())).join("v");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (Vault::open(&dir).unwrap(), dir)
    }

    // spec: BOOK-006
    #[test]
    fn the_numbers_and_levels_of_the_sections_follow_the_reading_order() {
        let (v, d) = vault("numbers", &[("T/index.md", "intro\n## A heading\n"), ("T/a.md", "# a\n\n## One\n### Two\n## Three\n"), ("T/P/index.md", "## Folder note heading\n"), ("T/P/x.md", "## X\n"), ("T/z.md", "### Deep first\n")]);
        let plan = book::plan(&v, Path::new("T")).unwrap();
        let secs = sections(&v, &plan, true);
        let lines = plan.lines();
        let printed: Vec<(usize, Option<String>)> = secs.iter().filter(|s| s.kind != Kind::Intro).map(|s| (s.level, s.number.clone())).collect();
        let want: Vec<(usize, Option<String>)> = lines.iter().map(|l| (l.depth + 1, l.number.clone())).collect();
        assert_eq!(printed, want, "the sections are the lines of the reading order");
        let by = |t: &str| secs.iter().find(|s| s.title == t).unwrap();
        // the first `# a` says the title: its line is blank, so `## One` is the first heading
        assert_eq!(by("a").text.lines().next(), Some(""));
        assert_eq!(by("a").heads.iter().map(|h| (h.label.as_str(), h.level)).collect::<Vec<_>>(), [("1.1 One", 2), ("1.1.1 Two", 3), ("1.2 Three", 2)]);
        assert_eq!(by("a").heads.iter().filter(|h| h.listed).count(), 2, "the contents have the first level only");
        assert_eq!(by("z").heads[0].label, "3.1 Deep first", "a heading below nothing starts at 1 (LINK-014)");
        // a folder note's headings are not numbered and not in the contents; the part's chapters are
        let part = by("P");
        assert_eq!((part.number.as_deref(), part.heads[0].label.as_str(), part.heads[0].level, part.heads[0].listed), (Some("2"), "Folder note heading", 2, false));
        assert_eq!(by("x").number.as_deref(), Some("2.1"));
        assert_eq!(by("x").heads[0].label, "2.1.1 X");
        let intro = secs.iter().find(|s| s.kind == Kind::Intro).unwrap();
        assert_eq!((intro.heads[0].label.as_str(), intro.heads[0].level), ("A heading", 2), "below the chapters: smaller than a chapter title");
        // numbering off: no numbers anywhere
        let plain = sections(&v, &plan, false);
        assert!(plain.iter().all(|s| s.number.is_none() && s.numbers.is_empty()));
        assert_eq!(plain.iter().find(|s| s.title == "x").unwrap().heads[0].label, "X");
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }
}
