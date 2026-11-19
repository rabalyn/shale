//! The blocks of a note as LaTeX (TEX-008, TEX-015): paragraphs, quotes, lists, tasks, fenced code,
//! tables, rules and the embeds on their own line, with the files they need (pictures, drawings
//! made as vector PDF, embedded PDFs) collected for the work folder. Headings are the writer's
//! (`writer.rs`): what a heading becomes depends on the kind.

use super::blocks::{Block, Item, ListLine, QuoteLine};
use super::inline::{cell, inline, lead, one_line, picture_file, Look};
use super::sets::tex_escape;
use crate::drawing::Scene;
use crate::markdown::LinkSpan;
use crate::pdf::export::ExportOptions;
use crate::table::Col;
use crate::vault::FileKind;
use eframe::egui::{Color32, Vec2};

/// A new page (TEX-037, TEX-038).
pub const NEW_PAGE: &str = "\\newpage\n\n";

/// The environment an alignment wrapper becomes (TEX-039): `center` and `flushright`; justified
/// text is what LaTeX sets anyway, and left is no wrapper.
fn align_env(a: crate::markdown::Align) -> Option<&'static str> {
    match a {
        crate::markdown::Align::Center => Some("center"),
        crate::markdown::Align::Right => Some("flushright"),
        crate::markdown::Align::Left | crate::markdown::Align::Justify => None,
    }
}

/// The most pages of an embedded PDF that are included (as in the native export).
const MAX_PDF_PAGES: usize = 40;

/// A column whose longest cell is this long (in characters) gets the room that is left; shorter
/// ones are as wide as their text.
const LONG_CELL: usize = 28;

/// The deepest `itemize` or `enumerate` LaTeX allows.
const MAX_LIST_DEPTH: usize = 4;

/// Turns the blocks of one note into TeX; the files that belong to it are collected in the
/// [`super::inline::Files`] of its [`Look`].
pub struct Body<'a> {
    pub look: Look<'a>,
    pub opts: &'a ExportOptions,
    /// Tables and code may break across pages (`xltabular`, a breakable box): not in slides,
    /// boxes or columns.
    pub long_tables: bool,
}

impl<'a> Body<'a> {
    pub fn new(look: Look<'a>, opts: &'a ExportOptions, long_tables: bool) -> Self {
        Body { look, opts, long_tables }
    }

    /// A block that is not a heading.
    pub fn block(&self, b: &Block) -> String {
        match b {
            Block::Heading { .. } => String::new(),
            Block::Para(lines) => format!("{}\n\n", self.lines(lines)),
            Block::Quote(lines) => self.quote(lines),
            Block::List(lines) => self.list(lines),
            Block::Code { lang, lines } => code(lang, lines),
            Block::Table(t) => self.table(t),
            Block::Math(tex) => format!("\\[\n{}\n\\]\n\n", one_line(tex)),
            Block::Embed(link) => self.embed(link),
            Block::Rule => "\\par\\medskip\\noindent\\hrulefill\\par\\medskip\n\n".into(),
            Block::PageBreak => NEW_PAGE.into(),
            Block::AlignStart(a) => align_env(*a).map_or(String::new(), |e| format!("\\begin{{{e}}}\n")),
            Block::AlignEnd(a) => align_env(*a).map_or(String::new(), |e| format!("\\end{{{e}}}\n\n")),
        }
    }

    /// The lines of a paragraph, a line break between them (the reading view keeps them too).
    fn lines(&self, lines: &[String]) -> String {
        lines.iter().map(|l| inline(&self.look, l)).collect::<Vec<_>>().join(" \\newline\n")
    }

    fn quote(&self, lines: &[QuoteLine]) -> String {
        let (mut out, mut depth, mut fresh) = (String::new(), 0, true);
        for q in lines {
            while depth < q.depth {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("\\begin{quote}\n");
                depth += 1;
                fresh = true;
            }
            while depth > q.depth {
                out.push_str("\n\\end{quote}\n");
                depth -= 1;
                fresh = true;
            }
            if q.text.is_empty() {
                out.push_str("\n\n");
                fresh = true;
            } else {
                if !fresh {
                    out.push_str(" \\newline\n");
                }
                out.push_str(&inline(&self.look, &q.text));
                fresh = false;
            }
        }
        for _ in 0..depth {
            out.push_str("\n\\end{quote}\n");
        }
        out.push('\n');
        out
    }

    /// Nested lists by indentation: a line indented more than the one above opens a list inside
    /// the item above, one indented less closes lists. Task lines get a box as their mark.
    fn list(&self, lines: &[ListLine]) -> String {
        let mut out = String::new();
        // the open lists: how far their items are indented, and whether they count
        let mut open: Vec<(usize, bool)> = Vec::new();
        for l in lines {
            while open.last().is_some_and(|(i, _)| *i > l.indent) {
                let (_, numbered) = open.pop().unwrap_or_default();
                out.push_str(&format!("\\end{{{}}}\n", if numbered { "enumerate" } else { "itemize" }));
            }
            // a bullet after a number (or the other way round) at the same depth is another list
            let counts = match l.item {
                Item::Number(_) => Some(true),
                Item::Bullet => Some(false),
                Item::Task(_) => None,
            };
            if open.last().is_some_and(|(i, numbered)| *i == l.indent && counts.is_some_and(|c| c != *numbered)) {
                let (_, numbered) = open.pop().unwrap_or_default();
                out.push_str(&format!("\\end{{{}}}\n", if numbered { "enumerate" } else { "itemize" }));
            }
            let deeper = open.last().is_none_or(|(i, _)| *i < l.indent);
            if deeper && open.len() < MAX_LIST_DEPTH {
                let numbered = matches!(l.item, Item::Number(_));
                out.push_str(&format!("\\begin{{{}}}\n", if numbered { "enumerate" } else { "itemize" }));
                // a numbered list that does not start at 1 keeps its first number
                if let (Item::Number(n), true) = (&l.item, open.is_empty()) {
                    if *n > 1 {
                        out.push_str(&format!("\\setcounter{{enumi}}{{{}}}\n", n - 1));
                    }
                }
                open.push((l.indent, numbered));
            }
            let text = lead(inline(&self.look, &l.text));
            match l.item {
                Item::Task(done) => out.push_str(&format!("\\item[\\shaletask{{{}}}] {text}\n", if done { "×" } else { "" })),
                _ => out.push_str(&format!("\\item {text}\n")),
            }
        }
        for (_, numbered) in open.into_iter().rev() {
            out.push_str(&format!("\\end{{{}}}\n", if numbered { "enumerate" } else { "itemize" }));
        }
        out.push('\n');
        out
    }

    /// A table as `booktabs` rules and `tabularx` (`xltabular` when it may break across pages):
    /// the header row bold, the columns as wide as their text or, when the table has stored sizes
    /// (EDIT-031), in the proportions of those, the alignment of each column as written.
    fn table(&self, b: &crate::table::Block) -> String {
        let t = &b.table;
        let n = t.header.len();
        let column = self.opts.column();
        let stored = crate::table::stored_widths(&b.meta.sizes.cols, n, column);
        let longest = |c: usize| t.rows.iter().map(|r| r[c].chars().count()).chain([t.header[c].chars().count()]).max().unwrap_or(0);
        let align = |a: Col| match a {
            Col::Left => "\\raggedright",
            Col::Center => "\\centering",
            Col::Right => "\\raggedleft",
        };
        let letter = |a: Col| match a {
            Col::Left => 'l',
            Col::Center => 'c',
            Col::Right => 'r',
        };
        let (width, spec) = match &stored {
            Some(w) => {
                // every column takes its stored share of the table's width, which is the stored
                // width but never more than the text
                let total: f32 = w.iter().sum();
                let mut shares: Vec<f32> = w.iter().map(|x| x / total * n as f32).collect();
                let rest = n as f32 - shares[..n - 1].iter().sum::<f32>();
                shares[n - 1] = rest;
                let cols: String = (0..n).map(|c| format!(">{{\\hsize={:.4}\\hsize{}\\arraybackslash}}X", shares[c], align(t.aligns[c]))).collect();
                ((total / column).clamp(0.05, 1.0), cols)
            }
            None => {
                // a line break in a cell (`<br>`, TEX-029) needs a column of paragraphs
                let breaks = |c: usize| t.rows.iter().map(|r| &r[c]).chain([&t.header[c]]).any(|x| x.to_ascii_lowercase().contains("<br"));
                let mut wide: Vec<bool> = (0..n).map(|c| longest(c) >= LONG_CELL || breaks(c)).collect();
                if !wide.contains(&true) {
                    // there must be a column that takes the room that is left
                    if let Some(c) = (0..n).rev().max_by_key(|c| longest(*c)) {
                        wide[c] = true;
                    }
                }
                let cols: String = (0..n).map(|c| if wide[c] { format!(">{{{}\\arraybackslash}}X", align(t.aligns[c])) } else { letter(t.aligns[c]).to_string() }).collect();
                (1.0, cols)
            }
        };
        let env = if self.long_tables { "xltabular" } else { "tabularx" };
        let row = |cells: &[String], bold: bool| {
            let cells: Vec<String> = cells.iter().map(|c| cell(&self.look, c)).map(|c| if bold && !c.is_empty() { format!("\\textbf{{{c}}}") } else { c }).collect();
            format!("{} \\\\\n", cells.join(" & "))
        };
        *self.look.deferred.borrow_mut() = Some(Vec::new());
        let mut out = format!("\\begingroup\\renewcommand{{\\arraystretch}}{{1.2}}\\par\\medskip\n\\begin{{{env}}}{{{width:.3}\\linewidth}}{{{spec}}}\n\\toprule\n");
        out.push_str(&row(&t.header, true));
        out.push_str("\\midrule\n");
        if self.long_tables {
            out.push_str("\\endhead\n");
        }
        for r in &t.rows {
            out.push_str(&row(r, false));
        }
        out.push_str(&format!("\\bottomrule\n\\end{{{env}}}\n"));
        // the footnotes of the cells: `\footnote` prints nothing inside a table, so the marks were written
        // and their texts come here, the counter taken back to the first mark
        let later = self.look.deferred.borrow_mut().take().unwrap_or_default();
        if !later.is_empty() {
            out.push_str(&format!("\\addtocounter{{footnote}}{{-{}}}\n", later.len()));
            for t in later {
                out.push_str(&format!("\\stepcounter{{footnote}}\\footnotetext{{{t}}}\n"));
            }
        }
        out.push_str("\\par\\medskip\\endgroup\n\n");
        out
    }

    // ------------------------------------------------------------ embeds (TEX-015)

    /// An embed on its own line: a picture, a drawing (as a vector PDF), the pages of a PDF, each
    /// as big in proportion to the text as in the editor (PDF-005) with the caption below
    /// (PDF-004); a file that cannot be used is a line of text that says so.
    fn embed(&self, link: &LinkSpan) -> String {
        let caption = crate::caption::text(link);
        if !self.opts.embeds {
            return format!("{}\n\n", inline(&self.look, &format!("[[{}]]", link.target)));
        }
        let vault = self.look.vault;
        let Some(rel) = vault.resolve(&link.target, Some(self.look.rel)) else {
            return self.placeholder(&format!("missing: {}", link.target));
        };
        let abs = vault.abs(&rel);
        let column = self.opts.column();
        let kind = vault.file(&rel).map(|f| f.kind).unwrap_or(FileKind::Other);
        let frame = |file: &str, extra: &str, share: f32| format!("\\includegraphics[{extra}width={:.3}\\linewidth,height=0.8\\textheight,keepaspectratio]{{{file}}}", share.clamp(0.02, 1.0));
        let figure = |pictures: Vec<String>, caption: &str| {
            let caption = tex_escape(caption);
            format!("\\par\\medskip\\noindent\n{}\\par\\nopagebreak\\noindent{{\\footnotesize\\textit{{{caption}}}}}\\par\\medskip\n\n", pictures.join("\\par\n"))
        };
        if kind == FileKind::Drawing {
            let Ok(scene) = Scene::read_file(&abs) else { return self.placeholder(&format!("not readable: {}", link.target)) };
            let Some(b) = scene.bounds() else { return self.placeholder(&format!("empty drawing: {}", link.target)) };
            let b = b.expand(20.0);
            let share = crate::embed_size::drawing(Vec2::new(b.width(), b.height()), column).x / column;
            let named = self.look.files.borrow().named(&rel);
            let name = match named {
                Some(n) => n,
                None => {
                    let opts = ExportOptions { margins_mm: [20.0 * 25.4 / 72.0; 4], background: crate::pdf::export::Background::None, ..ExportOptions::for_drawing(&scene) };
                    let pdf = crate::pdf::export::export_drawing(&scene, &link.target, &opts, &crate::drawing::pictures::images(&scene, vault, &rel));
                    let Ok(pdf) = pdf else { return self.placeholder(&format!("not readable: {}", link.target)) };
                    self.keep(&rel, "drawing", "pdf", pdf)
                }
            };
            return figure(vec![frame(&name, "", share)], &caption);
        }
        if crate::media::is_image(&rel) {
            let Ok((w, h)) = image::image_dimensions(&abs) else { return self.placeholder(&format!("not readable: {}", link.target)) };
            let share = crate::embed_size::picture(Vec2::new(w as f32, h as f32), link.embed_width(), column).x / column;
            let named = self.look.files.borrow().named(&rel);
            let name = match named {
                Some(n) => n,
                None => {
                    let Some((ext, bytes)) = picture_file(&abs) else { return self.placeholder(&format!("not readable: {}", link.target)) };
                    self.keep(&rel, "picture", ext, bytes)
                }
            };
            return figure(vec![frame(&name, "", share)], &caption);
        }
        if crate::pdf::view::is_pdf(&rel) {
            let Ok(bytes) = crate::fsutil::read(&abs) else { return self.placeholder(&format!("not readable: {}", link.target)) };
            let Some(pages) = pdf_pages(&bytes) else { return self.placeholder(&format!("not readable: {}", link.target)) };
            let named = self.look.files.borrow().named(&rel);
            let name = named.unwrap_or_else(|| self.keep(&rel, "document", "pdf", bytes));
            let pictures = pages.iter().enumerate().map(|(n, (w, h))| frame(&name, &format!("page={},", n + 1), crate::embed_size::pdf_page(Vec2::new(*w, *h) * 1.5, column).x / column)).collect();
            return figure(pictures, &caption);
        }
        // a sound or a video cannot be on paper
        self.placeholder(&format!("{}: not in a PDF", link.target))
    }

    /// A line of red italic text in place of a file that could not be used.
    fn placeholder(&self, what: &str) -> String {
        let text = self.look.files.borrow_mut().missing(what);
        format!("\\par\\medskip\\noindent{text}\\par\\medskip\n\n")
    }

    /// Remember a file for the work folder.
    fn keep(&self, rel: &std::path::Path, what: &str, ext: &str, bytes: Vec<u8>) -> String {
        self.look.files.borrow_mut().keep(rel, what, ext, bytes)
    }
}

/// The sizes of the pages (up to [`MAX_PDF_PAGES`]) of a PDF in points; None when it cannot be read.
fn pdf_pages(bytes: &[u8]) -> Option<Vec<(f32, f32)>> {
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).ok()?;
    Some(pdf.pages().iter().take(MAX_PDF_PAGES).map(|p| p.render_dimensions()).collect())
}

// ---------------------------------------------------------------- fenced code

/// The light box fenced code is drawn on (TEX-030): the grey of code on paper, under the colours of
/// [`print_syntax`].
pub const CODE_PAPER: Color32 = Color32::from_rgb(244, 244, 246);

/// The colour a kind of code has on white paper: the reading view's colours, darkened.
fn print_syntax(kind: crate::syntax::Kind) -> Color32 {
    use crate::syntax::Kind;
    match kind {
        Kind::Keyword => Color32::from_rgb(125, 45, 170),
        Kind::String => Color32::from_rgb(40, 110, 30),
        Kind::Number | Kind::Constant => Color32::from_rgb(180, 80, 20),
        Kind::Comment => Color32::from_rgb(110, 115, 125),
        Kind::Function => Color32::from_rgb(25, 95, 190),
        Kind::Type => Color32::from_rgb(150, 110, 0),
        Kind::Operator => Color32::from_rgb(0, 125, 130),
    }
}

/// One line of code as TeX text: escaped, every space kept (`\ `), a tab four spaces.
fn code_line(line: &str) -> String {
    tex_escape(&line.replace('\t', "    ")).replace(' ', "\\ ")
}

/// A fenced block on its light box (TEX-030) in the monospaced font, a line per paragraph, each piece in the colour of its
/// kind (`syntax::highlight`, plain without a known language; TeX has no listings package here:
/// the text is escaped like all text, so a line of code can never end the block).
fn code(lang: &str, lines: &[String]) -> String {
    let text = lines.join("\n");
    let spans = crate::syntax::highlight(lang, &text);
    let mut out = String::from("\\begin{shalecode}\n");
    let mut at = 0;
    for line in lines {
        let mut cut = vec![];
        // the spans that start on this line, cut at its end
        for (r, kind) in spans.iter() {
            let (s, e) = (r.start.max(at), r.end.min(at + line.len()));
            if s < e && line.is_char_boundary(s - at) && line.is_char_boundary(e - at) {
                cut.push((s - at..e - at, *kind));
            }
        }
        let mut done = 0;
        out.push_str("\\mbox{}");
        for (r, kind) in cut {
            out.push_str(&code_line(&line[done..r.start]));
            let c = print_syntax(kind);
            out.push_str(&format!("\\textcolor[RGB]{{{},{},{}}}{{{}}}", c.r(), c.g(), c.b(), code_line(&line[r.clone()])));
            done = r.end;
        }
        out.push_str(&code_line(&line[done..]));
        out.push_str("\\par\n");
        at += line.len() + 1;
    }
    out.push_str("\\end{shalecode}\n\n");
    out
}
