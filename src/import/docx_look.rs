//! The look of a Word file (IMP-040, IMP-041, rabalyn/shale#338): pandoc keeps the text and its
//! structure, not the page setup, the fonts and sizes, the spacing, the alignment or the colours.
//! This module reads them from the file itself with roxmltree. The page and the body text go to the
//! note's properties (`print-*`, which the PDF export reads); the colours of runs go into the text
//! through a tagged copy of the file: pandoc's reader drops direct run formatting and a Lua filter
//! cannot read the file, so each coloured run gets private-use sentinels around its text in
//! `word/document.xml`, which survive pandoc as plain characters and become HTML afterwards
//! ([`colours_to_html`]). Page breaks (IMP-043) go the same way with a sentinel paragraph at the
//! place of each break ([`mark_breaks`], [`breaks_to_html`]). Nothing here fails the import: a file that cannot be read gives no look
//! and no colours.

use crate::pdf::tex::archive;
use roxmltree::{Document, Node};
use std::path::{Path, PathBuf};

/// The namespace of `w:` (WordprocessingML).
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// Marks the start of a tag (`c:RRGGBB` or `h:RRGGBB`, then [`TAG_END`]).
const OPEN: char = '\u{E000}';
const TAG_END: char = '\u{E001}';
/// Ends a text colour.
const CLOSE_COLOUR: char = '\u{E002}';
/// Ends a highlight.
const CLOSE_MARK: char = '\u{E003}';

/// Stands for a page break (IMP-043): in a paragraph of its own, or where a manual break was.
const BREAK: char = '\u{E004}';
/// What a break becomes in the note (the contract with the PDF export).
const BREAK_HTML: &str = "<div style=\"page-break-after: always;\"></div>";

/// Paper sizes in twips (portrait) and how close a page must be to match (IMP-040).
const PAPERS: [(&str, u32, u32); 4] = [("A4", 11906, 16838), ("A5", 8391, 11906), ("Letter", 12240, 15840), ("Legal", 12240, 20160)];
const PAPER_TOLERANCE: u32 = 20;

/// What was read from a Word file: each field is `None` when the file does not say.
#[derive(Debug, Default, PartialEq)]
pub struct Look {
    paper: Option<String>,
    margins: Option<String>,
    font: Option<&'static str>,
    font_size: Option<String>,
    line_spacing: Option<String>,
    paragraph_spacing: Option<String>,
    align: Option<&'static str>,
    heading_color: Option<String>,
    /// the lowest level of a Heading style that starts a page (IMP-043)
    break_before: Option<u8>,
    /// the document part was read: a Word import never prints a title of its own, and has a table of contents or not
    title_off: bool,
    toc: bool,
}

impl Look {
    /// The properties in the order of IMP-040, values without units. Empty for a file that could not be read.
    pub fn props(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut put = |k: &'static str, v: Option<String>| out.extend(v.map(|v| (k, v)));
        put("print-paper", self.paper.clone());
        put("print-margins", self.margins.clone());
        put("print-font", self.font.map(String::from));
        put("print-font-size", self.font_size.clone());
        put("print-line-spacing", self.line_spacing.clone());
        put("print-paragraph-spacing", self.paragraph_spacing.clone());
        put("print-align", self.align.map(String::from));
        // a colour starts with `#`, which YAML reads as a comment unless it is quoted
        put("print-heading-color", self.heading_color.as_deref().map(super::yaml));
        put("print-break-before", self.break_before.map(|n| n.to_string()));
        put("print-title", self.title_off.then(|| "false".to_string()));
        put("print-toc", (self.title_off && !self.toc).then(|| "false".to_string()));
        out
    }
}

/// A number of at most two decimals without trailing zeros (`8`, `1.15`, `10.5`).
fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Twips as millimetres with one decimal, `.0` dropped (`1134` → `20`, `12240` → `215.9`).
fn mm(twips: f64) -> String {
    let v = (twips * 25.4 / 1440.0 * 10.0).round() / 10.0;
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The paper as the contract writes it: a known size by name when it stands upright, else `<W>x<H>mm`.
fn paper(w: f64, h: f64) -> String {
    let near = |a: f64, b: u32| (a - b as f64).abs() <= PAPER_TOLERANCE as f64;
    let known = PAPERS.iter().find(|(_, pw, ph)| (near(w, *pw) && near(h, *ph)) || (near(w, *ph) && near(h, *pw)));
    match known {
        Some((name, ..)) if w <= h => (*name).to_string(),
        _ => format!("{}x{}mm", mm(w), mm(h)),
    }
}

fn is_w(n: Node, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(W)
}

/// The first child of `n` named `name` in `w:`.
fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is_w(*c, name))
}

fn val<'a>(n: Node<'a, '_>, attr: &str) -> Option<&'a str> {
    n.attribute((W, attr))
}

/// The font names of a theme: (major, minor) `a:latin typeface`.
fn theme_fonts(theme: &Document) -> (Option<String>, Option<String>) {
    let face = |part: &str| {
        let p = theme.descendants().find(|n| n.is_element() && n.tag_name().name() == part)?;
        let latin = p.children().find(|n| n.is_element() && n.tag_name().name() == "latin")?;
        latin.attribute("typeface").filter(|t| !t.is_empty()).map(String::from)
    };
    (face("majorFont"), face("minorFont"))
}

/// A colour of a theme (`accent1`, `dk2`, …; Word's `text1` and `background1` names too) as `RRGGBB`.
fn theme_colour(theme: &Document, name: &str) -> Option<String> {
    let slot = match name {
        "text1" => "dk1",
        "background1" => "lt1",
        "text2" => "dk2",
        "background2" => "lt2",
        n => n,
    };
    let scheme = theme.descendants().find(|n| n.is_element() && n.tag_name().name() == "clrScheme")?;
    let entry = scheme.children().find(|n| n.is_element() && n.tag_name().name() == slot)?;
    let c = entry.children().find(|n| n.is_element())?;
    let hex = c.attribute("val").or_else(|| c.attribute("lastClr"))?;
    hex6(hex)
}

/// `hex` as upper case `RRGGBB` when it is six hex digits.
fn hex6(hex: &str) -> Option<String> {
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_ascii_uppercase())
}

/// The generic family of a font name: mono, then sans, then serif; `None` for a name not known.
pub fn font_family(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if any(&["courier", "consolas", "menlo", "monaco", "mono", "lucida console", "cascadia"]) {
        Some("mono")
    } else if any(&[
        "calibri",
        "arial",
        "aptos",
        "helvetica",
        "segoe",
        "verdana",
        "tahoma",
        "open sans",
        "sans",
        "trebuchet",
        "candara",
        "corbel",
        "lato",
        "roboto",
        "franklin",
        "gill",
        "century gothic",
        "avenir",
        "futura",
        "ubuntu",
        "inter",
    ]) {
        Some("sans")
    } else if any(&["times", "cambria", "georgia", "garamond", "book antiqua", "palatino", "serif", "constantia", "baskerville", "bookman", "century schoolbook", "minion", "charter", "didot"]) {
        Some("serif")
    } else {
        None
    }
}

/// Run and paragraph properties as they add up: the defaults of the document, then the style Normal.
#[derive(Default)]
struct Base {
    font: Option<String>,
    /// half points
    size: Option<f64>,
    line: Option<f64>,
    line_rule: Option<String>,
    after: Option<f64>,
    jc: Option<String>,
}

impl Base {
    /// Lay the `w:rPr` and `w:pPr` found in `holder` over what is there.
    fn apply(&mut self, holder: Node, fonts: &(Option<String>, Option<String>)) {
        if let Some(r) = child(holder, "rPr") {
            if let Some(f) = child(r, "rFonts") {
                // a theme font wins over a name, as in Word
                let themed = ["asciiTheme", "hAnsiTheme"].iter().find_map(|a| val(f, a)).map(|t| {
                    let major = t.starts_with("major");
                    if major {
                        fonts.0.clone()
                    } else {
                        fonts.1.clone()
                    }
                });
                let named = ["ascii", "hAnsi"].iter().find_map(|a| val(f, a)).map(|s| Some(s.to_string()));
                if let Some(font) = themed.or(named) {
                    self.font = font;
                }
            }
            if let Some(s) = child(r, "sz").and_then(|s| val(s, "val")).and_then(|v| v.parse().ok()) {
                self.size = Some(s);
            }
        }
        if let Some(p) = child(holder, "pPr") {
            if let Some(s) = child(p, "spacing") {
                if let Some(l) = val(s, "line").and_then(|v| v.parse().ok()) {
                    self.line = Some(l);
                    self.line_rule = val(s, "lineRule").map(String::from);
                }
                if let Some(a) = val(s, "after").and_then(|v| v.parse().ok()) {
                    self.after = Some(a);
                }
            }
            if let Some(j) = child(p, "jc").and_then(|j| val(j, "val")) {
                self.jc = Some(j.to_string());
            }
        }
    }
}

/// The look from the text of `word/document.xml`, `word/styles.xml` and `word/theme/theme1.xml`
/// (the last two optional). A part that does not parse counts as missing.
pub fn read(document: &str, styles: Option<&str>, theme: Option<&str>) -> Look {
    let mut look = Look::default();
    let Ok(doc) = Document::parse(document) else { return look };
    look.title_off = true;
    look.toc = has_toc(&doc);
    // the last section holds the page setup of the document
    if let Some(sect) = doc.descendants().rfind(|n| is_w(*n, "sectPr")) {
        let num = |n: Node, a: &str| val(n, a).and_then(|v| v.parse::<f64>().ok());
        if let Some(pg) = child(sect, "pgSz") {
            if let (Some(mut w), Some(mut h)) = (num(pg, "w"), num(pg, "h")) {
                if val(pg, "orient") == Some("landscape") && w < h {
                    std::mem::swap(&mut w, &mut h);
                }
                look.paper = Some(paper(w, h));
            }
        }
        if let Some(m) = child(sect, "pgMar") {
            let sides: Option<Vec<String>> = ["top", "right", "bottom", "left"].iter().map(|a| num(m, a).map(mm)).collect();
            look.margins = sides.map(|s| s.join(" "));
        }
    }
    let theme = theme.and_then(|t| Document::parse(t).ok());
    let Some(styles) = styles.and_then(|s| Document::parse(s).ok()) else { return look };
    let fonts = theme.as_ref().map(theme_fonts).unwrap_or((None, None));
    let mut base = Base::default();
    if let Some(defaults) = styles.descendants().find(|n| is_w(*n, "docDefaults")) {
        for part in ["rPrDefault", "pPrDefault"] {
            if let Some(d) = child(defaults, part) {
                base.apply(d, &fonts);
            }
        }
    }
    let style = |id: &str| styles.descendants().find(|n| is_w(*n, "style") && val(*n, "styleId") == Some(id));
    if let Some(normal) = style("Normal") {
        base.apply(normal, &fonts);
    }
    look.font = base.font.as_deref().and_then(font_family);
    look.font_size = base.size.map(|s| trim_num(s / 2.0));
    let size_pt = base.size.map(|s| s / 2.0);
    look.line_spacing = match (base.line, base.line_rule.as_deref()) {
        (Some(l), None | Some("auto")) => Some(trim_num(l / 240.0)),
        (Some(l), Some(_)) => size_pt.filter(|s| *s > 0.0).map(|s| trim_num(l / 20.0 / s)),
        _ => None,
    };
    look.paragraph_spacing = base.after.map(|a| trim_num(a / 20.0));
    look.align = Some(if matches!(base.jc.as_deref(), Some("both" | "distribute")) { "justify" } else { "left" });
    look.heading_color = style("Heading1").and_then(|h| child(h, "rPr")).and_then(|r| child(r, "color")).and_then(|c| {
        let by_val = val(c, "val").and_then(hex6);
        by_val.or_else(|| val(c, "themeColor").and_then(|t| theme.as_ref().and_then(|th| theme_colour(th, t))))
    });
    look.heading_color = look.heading_color.filter(|c| c != "000000").map(|c| format!("#{c}"));
    look.break_before = (1..=9).find(|n| styles.descendants().any(|s| is_w(s, "style") && heading_level(s) == Some(*n) && style_page_break(&styles, s) == Some(true)));
    look
}

/// Whether the document has a table of contents: a `TOC` field or the content control of one.
fn has_toc(doc: &Document) -> bool {
    let toc = |instruction: &str| format!(" {} ", instruction.trim()).contains(" TOC ") || instruction.split_whitespace().next() == Some("TOC");
    doc.descendants().any(|n| {
        if is_w(n, "instrText") {
            return toc(n.text().unwrap_or(""));
        }
        if is_w(n, "fldSimple") {
            return toc(val(n, "instr").unwrap_or(""));
        }
        is_w(n, "docPartGallery") && val(n, "val") == Some("Table of Contents")
    })
}

/// Word's highlight names as hex.
fn highlight_hex(name: &str) -> Option<&'static str> {
    Some(match name {
        "yellow" => "FFFF00",
        "green" => "00FF00",
        "cyan" => "00FFFF",
        "magenta" => "FF00FF",
        "blue" => "0000FF",
        "red" => "FF0000",
        "darkBlue" => "000080",
        "darkCyan" => "008080",
        "darkGreen" => "008000",
        "darkMagenta" => "800080",
        "darkRed" => "800000",
        "darkYellow" => "808000",
        "darkGray" => "808080",
        "lightGray" => "C0C0C0",
        "black" => "000000",
        "white" => "FFFFFF",
        _ => return None,
    })
}

/// A change to the text of `document.xml`: the bytes `start..end` become `with`.
struct Edit {
    start: usize,
    end: usize,
    with: String,
}

/// The text of `document` with sentinels around the text of every run that has its own text colour
/// or a highlight other than plain yellow (which pandoc keeps as `==`), and that highlight's
/// element taken out so that pandoc does not also mark it. Whitespace at the ends of a text stays
/// outside, so that Markdown emphasis still works. `None` when there is nothing to tag or the text
/// does not parse.
pub fn tag_runs(document: &str) -> Option<String> {
    let doc = Document::parse(document).ok()?;
    let mut edits: Vec<Edit> = Vec::new();
    for run in doc.descendants().filter(|n| is_w(*n, "r")) {
        let Some(pr) = child(run, "rPr") else { continue };
        let colour = child(pr, "color").and_then(|c| val(c, "val")).and_then(hex6).filter(|c| c != "000000");
        let named = child(pr, "highlight").and_then(|h| val(h, "val").map(|v| (h, v)));
        let highlight = match named {
            // plain yellow stays pandoc's own `==`
            Some((_, "yellow")) => None,
            Some((h, name)) if highlight_hex(name).is_some() => {
                edits.push(Edit { start: h.range().start, end: h.range().end, with: String::new() });
                highlight_hex(name).map(String::from)
            }
            _ => child(pr, "shd").and_then(|s| val(s, "fill")).and_then(hex6),
        };
        let (mut open, mut close) = (String::new(), String::new());
        if let Some(c) = &colour {
            open.push_str(&format!("{OPEN}c:{c}{TAG_END}"));
            close.insert(0, CLOSE_COLOUR);
        }
        if let Some(h) = &highlight {
            open.push_str(&format!("{OPEN}h:{h}{TAG_END}"));
            close.insert(0, CLOSE_MARK);
        }
        if open.is_empty() {
            continue;
        }
        for t in run.children().filter(|c| is_w(*c, "t")) {
            let Some(text) = t.first_child().filter(|c| c.is_text()) else { continue };
            let range = text.range();
            let raw = &document[range.clone()];
            let body = raw.trim();
            if body.is_empty() {
                continue;
            }
            let lead = raw.len() - raw.trim_start().len();
            let from = range.start + lead;
            edits.push(Edit { start: from, end: from + body.len(), with: format!("{open}{body}{close}") });
        }
    }
    apply(document, edits)
}

/// `document` with the edits made, `None` when there are none.
fn apply(document: &str, mut edits: Vec<Edit>) -> Option<String> {
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.start));
    let mut out = document.to_string();
    for e in edits {
        out.replace_range(e.start..e.end, &e.with);
    }
    Some(out)
}

/// The paragraph that stands for a break in the tagged copy.
fn break_paragraph() -> String {
    format!("<w:p><w:r><w:t>{BREAK}</w:t></w:r></w:p>")
}

/// Whether a `w:pageBreakBefore`-like toggle is on: no `w:val` means on.
fn flag(n: Node) -> bool {
    !matches!(val(n, "val"), Some("0" | "false" | "off"))
}

/// `Heading1` … `Heading9`, or `heading 1` … as a name: the level.
fn level_of(s: &str) -> Option<u8> {
    let rest = s.to_lowercase().strip_prefix("heading")?.trim().to_string();
    rest.parse::<u8>().ok().filter(|n| (1..=9).contains(n) && rest.len() == 1)
}

/// The level of a style that is one of Word's Heading styles, by its id or its name.
fn heading_level(style: Node) -> Option<u8> {
    let named = child(style, "name").and_then(|n| val(n, "val")).and_then(level_of);
    val(style, "styleId").and_then(level_of).or(named)
}

/// Whether a paragraph style has `pageBreakBefore`, itself or through `basedOn` (the nearest
/// statement wins); `None` when no style on the way says.
fn style_page_break(styles: &Document, style: Node) -> Option<bool> {
    let mut at = style;
    for _ in 0..16 {
        if let Some(f) = child(at, "pPr").and_then(|p| child(p, "pageBreakBefore")) {
            return Some(flag(f));
        }
        let parent = child(at, "basedOn").and_then(|b| val(b, "val"))?;
        at = styles.descendants().find(|n| is_w(*n, "style") && val(*n, "styleId") == Some(parent))?;
    }
    None
}

/// The text of `document` with a sentinel paragraph at each place that starts a new page (IMP-043):
/// a `w:br` of type page (the run is split so that the sentinel stands between two runs and keeps
/// out of any emphasis), a paragraph with `w:pageBreakBefore` (directly or through its style, except
/// a Heading style, which `print-break-before` covers) and a paragraph whose `w:pPr` holds the
/// `w:sectPr` of a section that begins on a new page. Not in tables, text boxes and deleted text,
/// which have no page of their own. `None` when there is none or the text does not parse.
pub fn mark_breaks(document: &str, styles: Option<&str>) -> Option<String> {
    let doc = Document::parse(document).ok()?;
    let styles = styles.and_then(|s| Document::parse(s).ok());
    let nested = |n: Node| n.ancestors().any(|a| is_w(a, "tc") || is_w(a, "txbxContent") || is_w(a, "del"));
    let mut edits: Vec<Edit> = Vec::new();
    for n in doc.descendants().filter(|n| is_w(*n, "br") && val(*n, "type") == Some("page")) {
        let Some(run) = n.parent().filter(|r| is_w(*r, "r")) else { continue };
        if nested(n) {
            continue;
        }
        let rpr = child(run, "rPr").map(|p| document[p.range()].to_string()).unwrap_or_default();
        edits.push(Edit { start: n.range().start, end: n.range().end, with: format!("</w:r>{}<w:r>{rpr}", break_paragraph_run()) });
    }
    for p in doc.descendants().filter(|n| is_w(*n, "p")) {
        if nested(p) {
            continue;
        }
        let ppr = child(p, "pPr");
        let style = ppr.and_then(|pr| child(pr, "pStyle")).and_then(|s| val(s, "val")).and_then(|id| styles.as_ref()?.descendants().find(|n| is_w(*n, "style") && val(*n, "styleId") == Some(id)));
        let from_style = style.and_then(|s| style_page_break(styles.as_ref()?, s));
        let direct = ppr.and_then(|pr| child(pr, "pageBreakBefore")).map(flag);
        // a Heading style that starts a page is the property `print-break-before`, not a marker each
        let covered = from_style == Some(true) && style.and_then(heading_level).is_some();
        if !covered && direct.or(from_style).unwrap_or(false) {
            edits.push(Edit { start: p.range().start, end: p.range().start, with: break_paragraph() });
        }
        let new_page = ppr.and_then(|pr| child(pr, "sectPr")).is_some_and(|s| matches!(child(s, "type").and_then(|t| val(t, "val")), None | Some("nextPage" | "oddPage" | "evenPage")));
        if new_page {
            edits.push(Edit { start: p.range().end, end: p.range().end, with: break_paragraph() });
        }
    }
    apply(document, edits)
}

/// The sentinel as a run of its own, for the middle of a paragraph.
fn break_paragraph_run() -> String {
    format!("<w:r><w:t>{BREAK}</w:t></w:r>")
}

/// The Markdown that pandoc made from a tagged copy with every sentinel of [`mark_breaks`] turned
/// into a line `<div style="page-break-after: always;"></div>` of its own with a blank line before
/// and after. Breaks side by side become one, and one at the very start or end is dropped.
pub fn breaks_to_html(text: &str) -> String {
    if !text.contains(BREAK) {
        return text.to_string();
    }
    let parts: Vec<&str> = text.split(BREAK).map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut out = parts.join(&format!("\n\n{BREAK_HTML}\n\n"));
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// One piece of text with sentinels.
enum Piece<'a> {
    Text(&'a str),
    /// (is a highlight, `RRGGBB`)
    Open(bool, String),
    Close(bool),
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find([OPEN, CLOSE_COLOUR, CLOSE_MARK]) {
        if at > 0 {
            out.push(Piece::Text(&rest[..at]));
        }
        let c = rest[at..].chars().next().expect("found");
        rest = &rest[at + c.len_utf8()..];
        match c {
            CLOSE_COLOUR => out.push(Piece::Close(false)),
            CLOSE_MARK => out.push(Piece::Close(true)),
            _ => {
                // `c:RRGGBB` and the end of the tag; anything else is a stray sentinel
                let tag = rest.find(TAG_END).map(|e| (&rest[..e], e)).filter(|(t, _)| t.len() == 8 && (t.starts_with("c:") || t.starts_with("h:")) && hex6(&t[2..]).is_some());
                if let Some((t, e)) = tag {
                    out.push(Piece::Open(t.starts_with('h'), t[2..].to_string()));
                    rest = &rest[e + TAG_END.len_utf8()..];
                }
            }
        }
    }
    out.push(Piece::Text(rest));
    out
}

/// The Markdown that pandoc made from a tagged copy, with the sentinels turned into
/// `<span style="color: #RRGGBB;">…</span>` and `<mark style="background: #RRGGBB;">…</mark>`
/// (EDIT-028). Runs side by side with the same colour become one span, a closing that has no
/// opening is dropped and an opening that is never closed is closed at the end.
pub fn colours_to_html(text: &str) -> String {
    let all = pieces(text);
    let mut out = String::with_capacity(text.len());
    // what is open: (highlight, colour)
    let mut stack: Vec<(bool, String)> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        match &all[i] {
            Piece::Text(t) => out.push_str(t),
            Piece::Open(mark, c) => {
                stack.push((*mark, c.clone()));
                if *mark {
                    out.push_str(&format!("<mark style=\"background: #{c};\">"));
                } else {
                    out.push_str(&format!("<span style=\"color: #{c};\">"));
                }
            }
            Piece::Close(mark) => {
                if stack.last().is_some_and(|(m, _)| m == mark) {
                    let (_, c) = stack.pop().expect("checked");
                    // the next piece opens the same thing again: carry on in the same element
                    if let Some(Piece::Open(m2, c2)) = all.get(i + 1) {
                        if m2 == mark && *c2 == c {
                            stack.push((*mark, c));
                            i += 2;
                            continue;
                        }
                    }
                    out.push_str(if *mark { "</mark>" } else { "</span>" });
                }
            }
        }
        i += 1;
    }
    while let Some((mark, _)) = stack.pop() {
        out.push_str(if mark { "</mark>" } else { "</span>" });
    }
    // the end of a tag whose start was not one
    out.retain(|c| c != TAG_END);
    out
}

// ---------------------------------------------------------------- the zip of the tagged copy

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The files below `dir` as (name with `/`, path), `[Content_Types].xml` first, the rest sorted.
fn files_of(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            if e.file_type().map_err(|e| e.to_string())?.is_dir() {
                walk(&e.path(), &format!("{name}/"), out)?;
            } else {
                out.push((name, e.path()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort_by(|a, b| (a.0 != "[Content_Types].xml", &a.0).cmp(&(b.0 != "[Content_Types].xml", &b.0)));
    Ok(out)
}

/// Write the files below `dir` as a zip at `to`, stored without compression (the file is read
/// once by pandoc and then removed).
pub fn write_zip(dir: &Path, to: &Path) -> Result<(), String> {
    let files = files_of(dir)?;
    if files.len() > u16::MAX as usize {
        return Err("too many files".into());
    }
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    // 1980-01-01 00:00: the same bytes for the same files
    let (time, date) = (0u16, 0x0021u16);
    for (name, path) in &files {
        let data = crate::fsutil::read(path).map_err(|e| e.to_string())?;
        let size = u32::try_from(data.len()).map_err(|_| "a file is too big".to_string())?;
        let at = u32::try_from(out.len()).map_err(|_| "the archive is too big".to_string())?;
        let crc = crc32(&data);
        let n = name.as_bytes();
        // flags 0x0800: the names are UTF-8
        let head = |sig: u32, central: bool| {
            let mut h = sig.to_le_bytes().to_vec();
            if central {
                h.extend(20u16.to_le_bytes());
            }
            for v in [20u16, 0x0800, 0, time, date] {
                h.extend(v.to_le_bytes());
            }
            for v in [crc, size, size] {
                h.extend(v.to_le_bytes());
            }
            h.extend((n.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h
        };
        out.extend(head(0x0403_4b50, false));
        out.extend(n);
        out.extend(&data);
        central.extend(head(0x0201_4b50, true));
        // comment length, disk, internal and external attributes, offset of the local header
        for v in [0u16, 0, 0] {
            central.extend(v.to_le_bytes());
        }
        central.extend(0u32.to_le_bytes());
        central.extend(at.to_le_bytes());
        central.extend(n);
    }
    let start = u32::try_from(out.len()).map_err(|_| "the archive is too big".to_string())?;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, files.len() as u16, files.len() as u16] {
        out.extend(v.to_le_bytes());
    }
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    crate::fsutil::write_atomic(to, &out).map_err(|e| e.to_string())
}

/// What one Word file gives: its look, and the path of a tagged copy when there was a colour to tag.
pub struct Analysis {
    pub look: Look,
    pub tagged: Option<PathBuf>,
}

/// Read the look of `docx` and, when it has coloured runs or page breaks, write the tagged copy `tagged.docx`
/// in `work` (an existing folder). The original is not touched. Never fails: what cannot be done
/// is left out.
pub fn analyse(docx: &Path, work: &Path) -> Analysis {
    let none = Analysis { look: Look::default(), tagged: None };
    let Ok(bytes) = crate::fsutil::read(docx) else { return none };
    let dir = work.join("unzipped");
    if std::fs::create_dir_all(&dir).is_err() || archive::extract(&bytes, &dir).is_err() {
        return none;
    }
    let part = |p: &str| crate::fsutil::read_to_string(dir.join(p)).ok();
    let Some(document) = part("word/document.xml") else { return none };
    let look = read(&document, part("word/styles.xml").as_deref(), part("word/theme/theme1.xml").as_deref());
    let marked = mark_breaks(&document, part("word/styles.xml").as_deref());
    let tagged = tag_runs(marked.as_deref().unwrap_or(&document)).or(marked).and_then(|tagged| {
        crate::fsutil::write_atomic(&dir.join("word/document.xml"), tagged.as_bytes()).ok()?;
        let to = work.join("tagged.docx");
        write_zip(&dir, &to).ok()?;
        Some(to)
    });
    Analysis { look, tagged }
}
