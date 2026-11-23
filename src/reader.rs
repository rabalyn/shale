//! Reading view: renders a note without markdown syntax. Links are clickable (blue = exists,
//! red = missing), checkboxes toggle, and `![[drawing.excalidraw]]` embeds show the drawing.

use crate::colour_tags;
use crate::drawing::{self, Scene};
use crate::editor::{char_under, heading_scale, FOOTNOTE_TEXT, SCRIPT_SCALE};
use crate::fold::{FoldCmd, FoldKind};
use crate::fold_ui::{FoldUi, Frame, ReaderFolds, Widget};
use crate::markdown::{self, LinkSpan};
use crate::syntax;
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Sense, Stroke, TextFormat, Ui, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub enum ReaderAction {
    OpenLink {
        target: String,
        /// the heading the link names (LINK-015)
        heading: Option<String>,
        new_tab: bool,
    },
    ToggleTask(usize),
    OpenFile(PathBuf),
    /// A web link: open in the default browser.
    OpenUrl(String),
    /// A date of a property card: show this day (a day number, `calendar::time`) in the calendar (EDIT-108).
    OpenDay(i64),
    /// "Copy into the vault" on an outside embed: the link target whose file is to be copied (MEDIA-021)
    CopyOutside(String),
}

/// What the reading view shows besides the text: the numbers of the headings by line (LINK-014)
/// and the line of a heading to scroll to (LINK-015).
#[derive(Default)]
pub struct Extras<'a> {
    pub numbers: HashMap<usize, String>,
    pub scroll_to: Option<usize>,
    /// how the front matter is drawn (CFG-050)
    pub properties: crate::config::PropertiesView,
    /// the folds of the note, shared with its editor (EDIT-095)
    pub folds: Option<RefCell<ReaderFolds<'a>>>,
}

impl Extras<'_> {
    /// The numbers of the headings of `text` by line; none when numbering is off for the note
    /// (`vault_default` is the setting, the front matter can override it, CFG-040).
    pub fn numbers_of(text: &str, vault_default: bool) -> HashMap<usize, String> {
        if !markdown::headings_numbered(text, vault_default) {
            return HashMap::new();
        }
        markdown::heading_numbers(text).into_iter().filter_map(|(line, _, _, n)| n.map(|n| (line, n))).collect()
    }
}

/// What the reading view draws embeds from.
pub struct Caches<'a> {
    pub scenes: &'a mut SceneCache,
    pub media: &'a crate::media::MediaPlayer,
}

/// Parsed drawings, re-read when the file changes.
#[derive(Default)]
pub struct SceneCache {
    map: HashMap<PathBuf, (Option<SystemTime>, Scene)>,
}

impl SceneCache {
    pub fn get(&mut self, vault: &Vault, rel: &Path) -> Option<&Scene> {
        let abs = vault.abs(rel);
        let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        let stale = self.map.get(rel).map(|(m, _)| *m != mtime).unwrap_or(true);
        if stale {
            let scene = Scene::read_file(&abs).ok()?;
            self.map.insert(rel.to_path_buf(), (mtime, scene));
        }
        self.map.get(rel).map(|(_, s)| s)
    }
    pub fn put(&mut self, rel: &Path, scene: Scene) {
        self.map.insert(rel.to_path_buf(), (None, scene));
    }
}

/// A drawing file, parsed once per file version and kept in egui's memory (used by the editor).
pub fn scene_cached(ctx: &egui::Context, abs: &Path) -> Option<std::sync::Arc<Scene>> {
    let mtime = std::fs::metadata(abs).and_then(|m| m.modified()).ok();
    let id = egui::Id::unique(("shale-scene", abs.to_path_buf(), mtime));
    if let Some(s) = ctx.data(|d| d.get_temp::<Option<std::sync::Arc<Scene>>>(id)) {
        return s;
    }
    let scene = Scene::read_file(abs).ok().map(std::sync::Arc::new);
    ctx.data_mut(|d| d.insert_temp(id, scene.clone()));
    scene
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Inline {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    highlight: bool,
    /// the innermost `<mark style="background: …">` and `<span style="color: …">` here (EDIT-028)
    bg: Option<[u8; 4]>,
    fg: Option<[u8; 4]>,
    /// inside `<u>` (EDIT-051)
    under: bool,
    /// inside `<sub>` or `<sup>`, the innermost of them
    script: Option<colour_tags::Kind>,
}

/// What a footnote link span stands for (EDIT-050): spans of the layout of a line that are not
/// links, but marks that jump to the other end of a footnote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Foot {
    /// the mark of a reference: goes to the definition
    #[default]
    Reference,
    /// the label of a definition: goes back to the first reference
    Definition,
}

const FOOT_REF: &str = "\u{1}footnote-reference";
const FOOT_DEF: &str = "\u{1}footnote-definition";

fn footnote_span(range: Range<usize>, label: &str, kind: Foot) -> LinkSpan {
    LinkSpan { target_range: range.clone(), range, target: label.to_string(), heading: Some(if kind == Foot::Reference { FOOT_REF } else { FOOT_DEF }.to_string()), alias: None, embed: false, markdown_style: false }
}

/// The footnote a link span of [`inline_painted`] stands for: what it is and its label.
pub(crate) fn footnote_of(l: &LinkSpan) -> Option<(Foot, &str)> {
    match l.heading.as_deref() {
        Some(FOOT_REF) => Some((Foot::Reference, &l.target)),
        Some(FOOT_DEF) => Some((Foot::Definition, &l.target)),
        _ => None,
    }
}

/// Where a line's text got its own colours (EDIT-029): its bytes in the job's text, the background
/// and the text colour as written. The PDF export draws these exactly as written.
pub(crate) type Paint = (Range<usize>, Option<[u8; 4]>, Option<[u8; 4]>);

/// A formula in a line (EDIT-048): where its TeX stands in the job's text (hidden when it was
/// typeset, with room for the formula before it; shown as code when not), what came of it and the
/// colour and size it is drawn in.
#[derive(Clone)]
pub(crate) struct MathAt {
    pub range: Range<usize>,
    pub formula: crate::math::Typeset,
    pub color: Color32,
    pub size: f32,
}

/// A picture embed that is drawn inside a line of text instead of its link text (EDIT-114,
/// PDF-030): where the file is, its own size in pixels and, on the screen, its texture.
#[derive(Clone)]
pub(crate) struct InlinePicture {
    pub path: PathBuf,
    pub size: Vec2,
    pub texture: Option<egui::TextureId>,
}

/// How high a picture inside a line is, in lines (EDIT-114, TEX-031).
pub(crate) const PICTURE_HEIGHT: f32 = 0.9;

/// How far the bottom of a picture inside a line is below the baseline, in lines.
pub(crate) const PICTURE_DROP: f32 = 0.2;

/// The size of a picture inside a line `line` points high: [`PICTURE_HEIGHT`] of it high, as wide
/// as its proportions say, scaled down with them to fit `max_width`.
pub(crate) fn inline_picture_size(natural: Vec2, line: f32, max_width: f32) -> Vec2 {
    let height = line * PICTURE_HEIGHT;
    let width = height * natural.x / natural.y.max(1.0);
    let k = if width > max_width { max_width / width } else { 1.0 };
    Vec2::new(width * k, height * k)
}

#[derive(Default)]
struct PicturesInScope {
    by_target: HashMap<String, InlinePicture>,
    max_width: f32,
}

thread_local! {
    /// The pictures that are drawn inside lines of the note being laid out (EDIT-114).
    static PICTURES: RefCell<PicturesInScope> = RefCell::new(PicturesInScope::default());
}

/// The pictures of a note by the target of the embed as written, and the width a picture is at
/// most, in force while this value lives: [`painted`] leaves the room of a picture in the line for
/// an embed named here, and shows every other as its link text. Without one nothing is named, so
/// the LaTeX export, the task list and the outline keep the link text. The pictures before come
/// back when it is dropped.
pub(crate) struct InlinePictures(PicturesInScope);

impl InlinePictures {
    pub fn set(by_target: HashMap<String, InlinePicture>, max_width: f32) -> Self {
        InlinePictures(PICTURES.with(|p| std::mem::replace(&mut *p.borrow_mut(), PicturesInScope { by_target, max_width })))
    }
}

impl Drop for InlinePictures {
    fn drop(&mut self) {
        PICTURES.with(|p| *p.borrow_mut() = std::mem::take(&mut self.0));
    }
}

/// The picture the embed of `target` is drawn as inside a line, and the width it is at most (see [`InlinePictures`]).
pub(crate) fn inline_picture(target: &str) -> Option<(InlinePicture, f32)> {
    PICTURES.with(|p| {
        let p = p.borrow();
        p.by_target.get(target).map(|pic| (pic.clone(), p.max_width))
    })
}

/// The link text of a picture inside a line as hidden text of a row `line` points high, with
/// `room` points for the picture before it. The first character is a section of its own: the room
/// is the leading space of the rest, which the text layout drops at the start of a row. A blank
/// is no place to break the text, and the text is two characters at least.
fn append_picture_room(job: &mut LayoutJob, shown: &str, room: f32, line: f32) {
    let mut text: String = shown.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    while text.chars().count() < 2 {
        text.push('_');
    }
    let format = TextFormat { font_id: FontId::proportional(crate::math::paint::HIDDEN_SIZE), color: Color32::TRANSPARENT, line_height: Some(line), ..Default::default() };
    let first = crate::math::paint::room_starts(&text);
    job.append(&text[..first], 0.0, format.clone());
    job.append(&text[first..], room, format);
}

/// A line as a layout job, the links in it and its formulas.
pub(crate) type InlineJob = (LayoutJob, Vec<(Range<usize>, LinkSpan, bool)>, Vec<MathAt>);

/// A line as a layout job, the links in it, its coloured text and its formulas.
pub(crate) type Painted = (LayoutJob, Vec<(Range<usize>, LinkSpan, bool)>, Vec<Paint>, Vec<MathAt>);

/// The text of a fenced code block, coloured by its language (EDIT-032; plain when there is none).
fn code_job(word: &str, code: &str, size: f32) -> LayoutJob {
    let format = |color| TextFormat { font_id: FontId::new(size, FontFamily::Monospace), color, ..Default::default() };
    let mut job = LayoutJob::default();
    let mut at = 0;
    for (r, kind) in syntax::highlight(word, code).iter() {
        job.append(&code[at..r.start], 0.0, format(theme::palette().code));
        job.append(&code[r.clone()], 0.0, format(theme::palette().syntax(*kind)));
        at = r.end;
    }
    job.append(&code[at..], 0.0, format(theme::palette().code));
    job
}

/// Turn one line of markdown into display text with formats and link positions.
pub(crate) fn inline_job(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool) -> InlineJob {
    let (job, spans, _, maths) = inline_painted(line, links, size, base_color, strike_all);
    (job, spans, maths)
}

/// `inline_job`, and the text that has colours of its own.
pub(crate) fn inline_painted(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool) -> Painted {
    painted(line, links, size, base_color, strike_all, false)
}

/// The text of a table cell as `inline_painted` lays it out, with `<br>` as a line break (EDIT-045).
pub(crate) fn cell_painted(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32) -> Painted {
    painted(line, links, size, base_color, false, true)
}

/// The length of the line break tag (`<br>`, `<br/>`, `<br />`, any case) at the start of `s`.
fn br_len(s: &str) -> Option<usize> {
    let rest = s.get(..3).filter(|t| t.starts_with('<') && t[1..].eq_ignore_ascii_case("br")).map(|_| &s[3..])?;
    let blanks = rest.len() - rest.trim_start().len();
    let rest = &rest[blanks..];
    let slash = usize::from(rest.starts_with('/'));
    rest[slash..].starts_with('>').then_some(3 + blanks + slash + 1)
}

fn painted(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool, breaks: bool) -> Painted {
    let mut job = LayoutJob::default();
    let mut paints: Vec<Paint> = Vec::new();
    let runs = colour_tags::find(line);
    let mut link_spans = Vec::new();
    let mut maths: Vec<MathAt> = Vec::new();
    let formulas = markdown::find_math(line);
    let tags = markdown::find_tags(line);
    let notes: Vec<(Range<usize>, String)> = markdown::footnote_refs_in_line(line).into_iter().filter(|(_, label)| markdown::footnote_is_defined(label)).collect();
    let mut st = Inline { strike: strike_all, ..Default::default() };
    let fmt = |st: &Inline, color: Color32| {
        // bold and italic in the font's own faces where it has them (CFG-039)
        let styled = crate::fonts::styled(st.bold && !st.code, st.italic && !st.code);
        let mut f = TextFormat {
            font_id: if st.code { FontId::new(size * 0.92, FontFamily::Monospace) } else { FontId::new(size, styled.family) },
            color: if st.code {
                theme::palette().code
            } else if st.bold {
                theme::palette().text_strong
            } else {
                color
            },
            italics: if st.code { st.italic } else { styled.fake_italic },
            line_height: Some(size * 1.5),
            ..Default::default()
        };
        if st.code {
            f.background = theme::palette().code_bg;
        }
        if st.highlight {
            f.background = theme::palette().highlight_bg;
        }
        if st.strike {
            f.strikethrough = Stroke::new(1.0, theme::palette().done);
            f.color = theme::palette().done;
        }
        if st.bold && (st.code || styled.fake_bold) {
            f.extra_letter_spacing = 0.3;
        }
        // a coloured highlight with readable text on it, and a font colour (EDIT-029)
        if let Some(bg) = st.bg {
            f.background = Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], bg[3]);
            f.color = if colour_tags::dark_text_on(bg) { Color32::BLACK } else { Color32::WHITE };
        }
        if let Some(fg) = st.fg {
            f.color = Color32::from_rgba_unmultiplied(fg[0], fg[1], fg[2], fg[3]);
        }
        if st.under {
            f.underline = Stroke::new(1.0, f.color);
        }
        if let Some(script) = st.script {
            f = script_format(f, script == colour_tags::Kind::Sup);
        }
        f
    };
    // web links ride along as link spans whose target is the URL
    let web: Vec<(LinkSpan, bool)> =
        crate::lists::find_web_links(line).into_iter().map(|w| (LinkSpan { range: w.range, target_range: w.label_range, target: w.url, heading: None, alias: Some(w.label), embed: false, markdown_style: true }, true)).collect();
    let b = line.as_bytes();
    let mut i = 0;
    // the text appended since the last look and the colours it was given
    let mut prev: (usize, Option<[u8; 4]>, Option<[u8; 4]>) = (0, None, None);
    let flush = |job: &LayoutJob, paints: &mut Vec<Paint>, prev: &mut (usize, Option<[u8; 4]>, Option<[u8; 4]>)| {
        let end = job.text.len();
        if (prev.1.is_some() || prev.2.is_some()) && end > prev.0 {
            match paints.last_mut() {
                Some(last) if last.0.end == prev.0 && last.1 == prev.1 && last.2 == prev.2 => last.0.end = end,
                _ => paints.push((prev.0..end, prev.1, prev.2)),
            }
        }
        *prev = (end, None, None);
    };
    while i < line.len() {
        flush(&job, &mut paints, &mut prev);
        // the tags of colours are not shown; the text between them gets the colour
        if let Some(r) = runs.iter().find(|r| r.open.start == i).map(|r| r.open.clone()).or_else(|| runs.iter().find(|r| r.close.start == i).map(|r| r.close.clone())) {
            i = r.end;
            continue;
        }
        (st.bg, st.fg) = colour_tags::colours_at(&runs, i);
        (st.under, st.script) = colour_tags::style_at(&runs, i);
        prev = (job.text.len(), st.bg, st.fg);
        if let Some((r, label)) = notes.iter().find(|(r, _)| r.start == i) {
            // the mark of a footnote: its label, raised, in the colour of links
            let start = job.text.len();
            let mut f = fmt(&st, theme::palette().link);
            f.color = theme::palette().link;
            job.append(label, 0.0, script_format(f, true));
            link_spans.push((start..job.text.len(), footnote_span(r.clone(), label, Foot::Reference), true));
            i = r.end;
            continue;
        }
        if let Some(m) = formulas.iter().find(|m| m.range.start == i) {
            // a formula: its TeX stays in the text, hidden, with room for the typeset formula before it
            // (EDIT-047); one that cannot be typeset is shown as code, and says why on hover (EDIT-049)
            let f = fmt(&st, base_color);
            let formula = crate::math::typeset(&line[m.tex.clone()], size, m.display);
            let start = job.text.len();
            match &formula {
                Ok(done) => crate::math::paint::append_hidden(&mut job, &line[m.range.clone()], done, size),
                Err(_) => {
                    let mut s2 = st;
                    s2.code = true;
                    job.append(&line[m.range.clone()], 0.0, fmt(&s2, base_color));
                }
            }
            maths.push(MathAt { range: start..job.text.len(), formula, color: f.color, size });
            i = m.range.end;
            continue;
        }
        if breaks {
            if let Some(len) = br_len(&line[i..]) {
                job.append("\n", 0.0, fmt(&st, base_color));
                i += len;
                continue;
            }
        }
        if let Some((l, ok)) = links.iter().chain(web.iter()).find(|(l, _)| l.range.start == i) {
            let shown = l.display();
            let start = job.text.len();
            if let Some((pic, max_width)) = l.embed.then(|| inline_picture(&l.target)).flatten() {
                // a picture inside the line: its link text stays, hidden, with room for the picture before it (EDIT-114)
                append_picture_room(&mut job, &shown, inline_picture_size(pic.size, size * 1.5, max_width).x, size * 1.5);
            } else {
                let color = if *ok { theme::palette().link } else { theme::palette().link_missing };
                let mut f = fmt(&st, color);
                f.color = if st.strike { theme::palette().done } else { color };
                f.underline = Stroke::new(1.0, color.gamma_multiply(0.5));
                job.append(&shown, 0.0, f);
            }
            link_spans.push((start..job.text.len(), l.clone(), *ok));
            i = l.range.end;
            continue;
        }
        if let Some((r, _)) = tags.iter().find(|(r, _)| r.start == i) {
            let mut f = fmt(&st, theme::palette().tag);
            f.background = theme::palette().tag_bg;
            job.append(&line[r.clone()], 0.0, f);
            i = r.end;
            continue;
        }
        let rest = &line[i..];
        if b[i] == b'`' {
            if let Some(e) = rest[1..].find('`') {
                let mut s2 = st;
                s2.code = true;
                let code = &rest[1..1 + e];
                let plain = fmt(&s2, base_color);
                let mut at = 0;
                for (r, kind) in syntax::highlight_inline(code) {
                    job.append(&code[at..r.start], 0.0, plain.clone());
                    job.append(&code[r.clone()], 0.0, TextFormat { color: theme::palette().syntax(kind), ..plain.clone() });
                    at = r.end;
                }
                job.append(&code[at..], 0.0, plain);
                i += e + 2;
                continue;
            }
        }
        if (rest.starts_with("**") || rest.starts_with("__")) && (st.bold || rest[2..].contains(&rest[..2])) {
            st.bold = !st.bold;
            i += 2;
            continue;
        }
        if rest.starts_with("~~") && (st.strike && !strike_all || rest[2..].contains("~~")) {
            st.strike = !st.strike;
            i += 2;
            continue;
        }
        if rest.starts_with("==") && (st.highlight || rest[2..].contains("==")) {
            st.highlight = !st.highlight;
            i += 2;
            continue;
        }
        if (b[i] == b'*' || b[i] == b'_') && (st.italic || (rest.len() > 1 && b[i + 1] != b' ' && rest[1..].contains(b[i] as char))) {
            let word_inner = b[i] == b'_' && i > 0 && b[i - 1].is_ascii_alphanumeric() && !st.italic;
            if !word_inner {
                st.italic = !st.italic;
                i += 1;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        job.append(&ch.to_string(), 0.0, fmt(&st, base_color));
        i += ch.len_utf8();
    }
    flush(&job, &mut paints, &mut prev);
    if job.text.is_empty() {
        job.append(" ", 0.0, fmt(&st, base_color));
    }
    // a formula that reaches below the text needs lines of text with more room below (EDIT-048)
    let extra = maths.iter().filter_map(|m| m.formula.as_ref().ok().map(|f| crate::math::paint::room_for(f, size).0)).fold(0.0, f32::max);
    if extra > 0.0 {
        for s in job.sections.iter_mut().filter(|s| !crate::math::paint::is_hidden_formula(&s.format)) {
            s.format.line_height = s.format.line_height.map(|h| h + extra);
        }
    }
    (job, link_spans, paints, maths)
}

/// `f` as sub or superscript (EDIT-051): smaller, placed at the top or the bottom of the row, which
/// needs the text not to have a line height of its own.
fn script_format(mut f: TextFormat, raised: bool) -> TextFormat {
    f.font_id.size *= SCRIPT_SCALE;
    f.line_height = None;
    f.valign = if raised { egui::Align::TOP } else { egui::Align::BOTTOM };
    f
}

/// Whether the text of a section of a layout is a sub or superscript (EDIT-051): `Some(true)` when
/// raised, `Some(false)` when lowered. Their format has no line height of its own (see
/// [`script_format`]), and no other text of a line has none. The TeX export reads it back (TEX-029).
pub(crate) fn script_of(f: &TextFormat) -> Option<bool> {
    f.line_height.is_none().then_some(f.valign == egui::Align::TOP)
}

/// Whether the text of a section of a layout is underlined by `<u>` (EDIT-051): its line has the
/// colour of the text; a link's line is a paler one. The TeX export reads it back (TEX-029).
pub(crate) fn underlined(f: &TextFormat) -> bool {
    f.underline.width > 0.0 && f.underline.color == f.color
}

/// The first line of a footnote definition (EDIT-050): its label raised, then the text a little
/// smaller. `line` is the whole line, `marker` the range of `[^label]:` in it.
pub(crate) fn footnote_def_painted(line: &str, label: &str, marker: &Range<usize>, links: &[(LinkSpan, bool)], size: f32, color: Color32) -> Painted {
    let lead = line[marker.end..].len() - line[marker.end..].trim_start().len();
    let by = marker.end + lead;
    let (job, spans, paints, maths) = inline_painted(&line[by..], &shift(links, by), size * FOOTNOTE_TEXT, color, false);
    let mark = TextFormat { font_id: FontId::new(size * FOOTNOTE_TEXT, crate::fonts::styled(false, false).family), color: theme::palette().link, ..Default::default() };
    let gap = mark.clone();
    let mut out = LayoutJob::default();
    out.append(label, 0.0, script_format(mark, true));
    out.append(" ", 0.0, gap);
    let head = out.text.len();
    let at = |r: &Range<usize>| r.start + head..r.end + head;
    out.text.push_str(&job.text);
    for sec in job.sections {
        out.sections.push(egui::text::LayoutSection { leading_space: sec.leading_space, byte_range: egui::text::ByteIndex(sec.byte_range.start.0 + head)..egui::text::ByteIndex(sec.byte_range.end.0 + head), format: sec.format });
    }
    let mut all = vec![(0..label.len(), footnote_span(0..label.len(), label, Foot::Definition), true)];
    all.extend(spans.into_iter().map(|(r, l, ok)| (at(&r), l, ok)));
    (out, all, paints.into_iter().map(|(r, bg, fg)| (at(&r), bg, fg)).collect(), maths.into_iter().map(|m| MathAt { range: at(&m.range), ..m }).collect())
}

/// An indented line that goes on with a footnote definition: its text a little smaller.
pub(crate) fn footnote_text_painted(line: &str, links: &[(LinkSpan, bool)], size: f32, color: Color32) -> Painted {
    let by = line.len() - line.trim_start().len();
    inline_painted(&line[by..], &shift(links, by), size * FOOTNOTE_TEXT, color, false)
}

/// Paint an inline job as a wrapped block and handle link clicks; its formulas are drawn in the
/// room they have in the text (EDIT-048), and one that failed says why on hover (EDIT-049).
/// `hint` gives an extra tooltip line for a link target (LINK-008, LINK-012).
fn inline_block(ui: &mut Ui, job: LayoutJob, spans: &[(Range<usize>, LinkSpan, bool)], maths: &[MathAt], hint: &dyn Fn(&str) -> Option<String>, out: &mut Option<ReaderAction>) -> egui::Response {
    inline_block_aligned(ui, job, spans, maths, hint, None, out)
}

/// [`inline_block`] laid out centred, to the right or justified over the width of the column when
/// `align` says so (PDF-028, EDIT-110).
fn inline_block_aligned(ui: &mut Ui, job: LayoutJob, spans: &[(Range<usize>, LinkSpan, bool)], maths: &[MathAt], hint: &dyn Fn(&str) -> Option<String>, align: Option<markdown::Align>, out: &mut Option<ReaderAction>) -> egui::Response {
    let mut job = job;
    job.wrap.max_width = ui.available_width();
    match align {
        Some(markdown::Align::Center) => job.halign = egui::Align::Center,
        Some(markdown::Align::Right) => job.halign = egui::Align::RIGHT,
        Some(markdown::Align::Justify) => job.justify = true,
        _ => {}
    }
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let aligned = matches!(align, Some(markdown::Align::Center | markdown::Align::Right | markdown::Align::Justify));
    let size = if aligned { Vec2::new(ui.available_width().max(galley.size().x), galley.size().y) } else { galley.size() };
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    // where the galley's origin is: its middle or its right edge when it is laid out around them
    let anchor = match align {
        Some(markdown::Align::Center) => Pos2::new(rect.center().x, rect.min.y),
        Some(markdown::Align::Right) => Pos2::new(rect.right(), rect.min.y),
        _ => rect.min,
    };
    ui.painter().galley(anchor, galley.clone(), theme::palette().text);
    paint_maths(ui, &galley, anchor, maths);
    paint_pictures(ui.painter(), &galley, anchor, spans);
    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
        if let Some(c) = char_under(&galley, anchor, hp) {
            let byte = markdown::char_to_byte(&galley.job.text, c);
            if let Some(Err(why)) = maths.iter().find(|m| m.range.contains(&byte)).map(|m| &m.formula) {
                resp.clone().on_hover_text(format!("This formula cannot be typeset: {why}"));
            }
            follow_span(ui, &resp, byte, spans, hint, out);
        }
    }
    resp
}

/// Draw the formulas of a laid-out line in the room their hidden TeX has (EDIT-048).
pub(crate) fn paint_maths(ui: &Ui, galley: &Galley, pos: Pos2, maths: &[MathAt]) {
    for m in maths {
        if let Ok(f) = &m.formula {
            let at = markdown::byte_to_char(&galley.job.text, m.range.start.min(galley.job.text.len()));
            crate::math::paint::paint_in_galley(ui.painter(), galley, pos, at, f, m.color);
        }
    }
}

/// Draw the pictures of a laid-out line in the room their hidden link text has (EDIT-114): the
/// spans of the line say where they are. A picture is [`PICTURE_HEIGHT`] of the row's line high,
/// its bottom [`PICTURE_DROP`] of it below the baseline of the text of the row.
pub(crate) fn paint_pictures(painter: &egui::Painter, galley: &Galley, pos: Pos2, spans: &[(Range<usize>, LinkSpan, bool)]) {
    for (range, l, _) in spans.iter().filter(|(_, l, _)| l.embed) {
        let Some((pic, max_width)) = inline_picture(&l.target) else { continue };
        let Some(texture) = pic.texture else { continue };
        let Some(line) = galley.job.format_at_byte(egui::text::ByteIndex(range.start)).line_height else { continue };
        let at = markdown::byte_to_char(&galley.job.text, range.start.min(galley.job.text.len()));
        let Some((x, baseline)) = crate::math::paint::place_in_row(galley, at, |height| height * 0.75) else { continue };
        let size = inline_picture_size(pic.size, line, max_width);
        let bottom = pos.y + baseline + line * PICTURE_DROP;
        let rect = egui::Rect::from_min_size(Pos2::new(pos.x + x, bottom - size.y), size);
        painter.image(texture, rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
}

/// A formula alone on its lines: centred in its own row in display style, or its TeX as code with
/// the reason on hover when it cannot be typeset (EDIT-047, EDIT-049).
fn math_block(ui: &mut Ui, tex: &str, base: f32) {
    match crate::math::typeset(tex, base * crate::math::DISPLAY_SCALE, true) {
        Ok(f) => {
            let size = crate::math::paint::size(&f);
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width.max(size.x), size.y + base * 0.8), Sense::hover());
            let left = rect.min.x + ((width - size.x) / 2.0).max(0.0);
            let pos = Pos2::new(left, rect.min.y + base * 0.4 + f.layout.ascent);
            crate::math::paint::paint(&ui.painter().with_clip_rect(ui.clip_rect()), &f, pos, theme::palette().text);
        }
        Err(why) => {
            egui::Frame::new().fill(theme::palette().code_bg).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(code_job("", tex.trim_end(), base * 0.9)).on_hover_text(format!("This formula cannot be typeset: {why}"));
            });
            ui.add_space(6.0);
        }
    }
}

/// Hover and click on the link at byte `byte` of a block's text (if there is one): the pointer,
/// the tooltip and the action (web links open the browser, notes open or are created).
fn follow_span(ui: &Ui, resp: &egui::Response, byte: usize, spans: &[(Range<usize>, LinkSpan, bool)], hint: &dyn Fn(&str) -> Option<String>, out: &mut Option<ReaderAction>) {
    if let Some((kind, label)) = spans.iter().find(|(r, ..)| r.contains(&byte)).and_then(|(_, l, _)| footnote_of(l)) {
        // a footnote mark goes to the other end of its footnote, in the next frame (EDIT-050)
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        resp.clone().on_hover_text(if kind == Foot::Reference { "Go to the footnote" } else { "Back to the text" });
        if resp.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(jump_id(), (kind, label.to_string())));
            ui.ctx().request_repaint();
        }
    } else if let Some((_, l, _)) = spans.iter().find(|(r, l, _)| r.contains(&byte) && crate::lists::is_web_url(&l.target)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(&l.target).color(theme::palette().link));
            ui.label(egui::RichText::new("Opens in your browser").small().color(theme::palette().muted));
        });
        if resp.clicked() || resp.middle_clicked() {
            *out = Some(ReaderAction::OpenUrl(l.target.clone()));
        }
    } else if let Some((_, l, ok)) = spans.iter().find(|(r, _, _)| r.contains(&byte)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let tip = if *ok { format!("Open “{}”", l.target) } else { format!("Create note “{}”", l.target) };
        let colour = if *ok { theme::palette().link } else { theme::palette().link_missing };
        let note = hint(&l.target);
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(tip).color(colour));
            if let Some(n) = &note {
                ui.label(egui::RichText::new(n).small().color(theme::palette().text));
            }
        });
        if resp.clicked() || resp.middle_clicked() {
            let new_tab = resp.middle_clicked() || ui.input(|i| i.modifiers.command);
            *out = Some(ReaderAction::OpenLink { target: l.target.clone(), heading: l.heading.clone(), new_tab });
        }
    }
}

/// One line of markdown with clickable links (used by the task list).
/// Returns a link action, and whether the text outside links was clicked.
pub fn inline_line(ui: &mut Ui, text: &str, vault: &Vault, from: &Path, base: f32, done: bool) -> (Option<ReaderAction>, bool) {
    let links: Vec<(LinkSpan, bool)> = markdown::find_links(text)
        .into_iter()
        .map(|l| {
            let ok = vault.exists(&l.target, Some(from));
            (l, ok)
        })
        .collect();
    let (job, spans, maths) = inline_job(text, &links, base, theme::palette().text, done);
    let mut out = None;
    let resp = inline_block(ui, job, &spans, &maths, &|t| vault.link_hint(t, Some(from)), &mut out);
    let plain_click = resp.clicked() && out.is_none();
    (out, plain_click)
}

/// Whether the embed at `range` of `text` has text around it on its line: one that is alone on its
/// line is a block of its own, which is not drawn inside a line (EDIT-114, PDF-030).
pub(crate) fn embed_is_in_a_line(text: &str, range: &Range<usize>) -> bool {
    let start = text[..range.start].rfind('\n').map_or(0, |p| p + 1);
    let end = text[range.end..].find('\n').map_or(text.len(), |p| range.end + p);
    text[start..end].trim().len() != range.len()
}

/// The picture embeds of `text` that stand inside a line and can be shown there, by their target:
/// a picture of the vault (EDIT-114, PDF-030). `resolve` finds the file of a link (a frame looks it
/// up in the background, MEDIA-024; an export asks the disk). `load` is given its file and says its
/// size in pixels and its texture; one it cannot read (or cannot yet) is left out and stays its link
/// text. A picture outside the vault counts (MEDIA-016).
pub(crate) fn pictures_in_lines(
    text: &str,
    links: &[(LinkSpan, bool)],
    vault: &Vault,
    resolve: impl Fn(&str) -> crate::vault::Shown,
    mut load: impl FnMut(&Path) -> Option<(Vec2, Option<egui::TextureId>)>,
) -> HashMap<String, InlinePicture> {
    let mut out = HashMap::new();
    for (l, _) in links.iter().filter(|(l, ok)| l.embed && *ok) {
        if out.contains_key(&l.target) || !embed_is_in_a_line(text, &l.range) {
            continue;
        }
        let Some(path) = resolve(&l.target).file(vault).filter(|(f, _)| crate::media::is_image(f)).map(|(_, abs)| abs) else { continue };
        if let Some((size, texture)) = load(&path) {
            out.insert(l.target.clone(), InlinePicture { path, size, texture });
        }
    }
    out
}

/// A click on a footnote mark waiting for the next frame: where it goes and the label.
fn jump_id() -> egui::Id {
    egui::Id::unique("shale-footnote-jump")
}

/// The line a waiting footnote jump goes to (EDIT-050): the definition, or the first reference.
/// The jump is taken: it is done once.
fn jump_target(ctx: &egui::Context, notes: &markdown::Footnotes) -> Option<usize> {
    let (kind, label): (Foot, String) = ctx.data_mut(|d| d.remove_temp(jump_id()))?;
    match kind {
        Foot::Reference => notes.def(&label).map(|d| d.lines.start),
        Foot::Definition => notes.first_ref(&label).map(|r| r.line),
    }
}

/// The thin dashed rule of a page-break marker with the small label "Page break" in its middle,
/// from `x0` to `x1` at height `y` (EDIT-065); the reading view and the editor's preview draw it.
pub(crate) fn paint_page_break(painter: &egui::Painter, x0: f32, x1: f32, y: f32) {
    let label = painter.layout_no_wrap("Page break".to_string(), egui::FontId::proportional(11.0), theme::palette().muted);
    let (mid, gap) = ((x0 + x1) / 2.0, label.size().x / 2.0 + 8.0);
    let stroke = egui::Stroke::new(1.0, theme::palette().border);
    for (a, b) in [(x0, mid - gap), (mid + gap, x1)] {
        if b > a {
            painter.extend(egui::Shape::dashed_line(&[Pos2::new(a, y), Pos2::new(b, y)], stroke, 5.0, 4.0));
        }
    }
    painter.galley(Pos2::new(mid - label.size().x / 2.0, y - label.size().y / 2.0), label, theme::palette().muted);
}

/// Draw a note. `look` is the text size and whether drawings get a dark canvas.
///
/// With `extras.folds` (EDIT-095) the blocks that are folded are drawn as their first line and a badge and
/// every foldable block has its marker; without, nothing is folded and no marker is drawn.
pub fn show(ui: &mut Ui, text: &str, vault: &Vault, rel: &Path, caches: &mut Caches, look: (f32, bool), extras: &Extras) -> Option<ReaderAction> {
    let mut folds = extras.folds.as_ref().map(|f| f.borrow_mut());
    let (base, dark_canvas) = look;
    let mut out = None;
    let notes = markdown::find_footnotes(text);
    let _defined = markdown::DefinedFootnotes::set(notes.defined());
    let target = jump_target(ui.ctx(), &notes);
    // the folds: fold all / unfold all by key first, then whatever hides a line that is asked for (a footnote
    // jump, a heading of the outline) is unfolded (EDIT-097)
    let props = folds.as_ref().map_or(crate::config::PropertiesView::Raw, |f| f.props);
    let mut frame = folds.as_mut().map(|f| {
        f.ui.widgets.clear();
        f.ui.begin(rel, text, props)
    });
    if let (Some(f), Some(fr)) = (folds.as_mut(), frame.as_mut()) {
        if let Some(cmd) = f.ui.request.filter(|c| matches!(c, FoldCmd::FoldAll | FoldCmd::UnfoldAll)) {
            f.ui.request = None;
            f.ui.apply(rel, fr, cmd, 0);
            *fr = f.ui.begin(rel, text, props);
        }
        for line in target.into_iter().chain(extras.scroll_to) {
            if f.ui.reveal_line(rel, fr, line) {
                *fr = f.ui.begin(rel, text, props);
            }
        }
    }
    let hint = |t: &str| vault.link_hint(t, Some(rel));
    let all_links: Vec<(LinkSpan, bool)> = markdown::find_links(text)
        .into_iter()
        .map(|l| {
            let ok = vault.exists(&l.target, Some(rel));
            (l, ok)
        })
        .collect();
    let pictures = pictures_in_lines(text, &all_links, vault, |t| vault.look_media(ui.ctx(), t, Some(rel)), |abs| crate::media::texture(ui.ctx(), abs).map(|t| (t.size_vec2(), Some(t.id()))));
    let _pictures = InlinePictures::set(pictures, ui.available_width());
    let fenced = markdown::fenced_code_lines(text);
    let lines = markdown::lines_with_offsets(text);
    let mut i = 0;
    // front matter
    if lines.first().map(|(_, l)| l.trim() == "---").unwrap_or(false) {
        if let Some(end) = lines.iter().skip(1).position(|(_, l)| l.trim() == "---") {
            let raw: Vec<&str> = lines[1..end + 1].iter().map(|(_, l)| *l).collect();
            crate::properties_ui::show(ui, text, &raw, extras.properties, vault, rel, &mut out);
            ui.add_space(8.0);
            i = end + 2;
        }
    }
    let tables = crate::table::find_tables(text);
    let formulas = markdown::find_math(text);
    let page_breaks = markdown::page_break_lines(text);
    let wrappers = markdown::align_blocks(text);
    // a very large note: what is far from the visible part is not drawn, a space of the height it had is left (EDIT-102)
    let heights = (lines.len() >= CULL_LINES).then(|| {
        let key = HeightsKey { text: crate::fold_ui::hash_of(text), width: ui.available_width().to_bits(), base: base.to_bits(), badges: frame.as_ref().map(|f| f.view.badges.clone()).unwrap_or_default() };
        let id = ui.scope_id().with(("reader-heights", rel));
        let shared = ui.data_mut(|d| d.get_temp::<std::sync::Arc<std::sync::Mutex<Heights>>>(id)).unwrap_or_default();
        {
            let mut h = shared.lock().unwrap_or_else(|e| e.into_inner());
            if h.key.as_ref() != Some(&key) {
                *h = Heights { key: Some(key), segs: HashMap::new() };
            }
        }
        ui.data_mut(|d| d.insert_temp(id, shared.clone()));
        shared
    });
    let wanted: Vec<usize> = target.into_iter().chain(extras.scroll_to).collect();
    while i < lines.len() {
        if let Some(h) = frame.as_ref().and_then(|f| f.view.hidden_at(i)) {
            // what a fold hides is not drawn (EDIT-095)
            i = h.end;
            continue;
        }
        let (start, top) = (i, ui.cursor().min.y);
        if let Some(h) = &heights {
            let known = h.lock().unwrap_or_else(|e| e.into_inner()).segs.get(&i).copied();
            if let Some((end, height)) = known {
                let clip = ui.clip_rect();
                let far = top + height < clip.min.y - clip.height() || top > clip.max.y + clip.height();
                if far && !wanted.iter().any(|t| (i..end).contains(t)) {
                    ui.add_space((height - ui.spacing().item_spacing.y).max(0.0));
                    i = end;
                    continue;
                }
            }
        }
        // the badge of a folded code block, drawn in its box (EDIT-098): where it is and whether it was clicked
        let mut in_box: Option<(egui::Rect, bool)> = None;
        'line: {
            let span = tables.iter().find(|b| b.lines.start == i).map_or(i + 1, |b| b.lines.end);
            if target.is_some_and(|t| (i..span).contains(&t)) {
                // the end of a footnote jump (EDIT-050): this is the line
                ui.scroll_to_rect(egui::Rect::from_min_size(ui.cursor().min, Vec2::new(1.0, base * 1.5)), Some(egui::Align::TOP));
            }
            if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
                table_block(ui, b, vault, rel, base, &hint, &mut out);
                i = b.lines.end;
                break 'line;
            }
            // a formula on lines of its own: `$$` lines, a `math` fence, `$$…$$` alone (EDIT-046)
            if let Some(m) = formulas.iter().find(|m| m.block && m.lines.start == i) {
                math_block(ui, &text[m.tex.clone()], base);
                i = m.lines.end;
                break 'line;
            }
            let (off, line) = lines[i];
            // the links are in the order of the text: only the ones from this line on
            let line_links: Vec<(LinkSpan, bool)> = all_links[all_links.partition_point(|(l, _)| l.range.start < off)..]
                .iter()
                .take_while(|(l, _)| l.range.start <= off + line.len())
                .filter(|(l, _)| l.range.start >= off && l.range.end <= off + line.len())
                .map(|(l, ok)| {
                    let mut l = l.clone();
                    l.range = l.range.start - off..l.range.end - off;
                    (l, *ok)
                })
                .collect();
            // fenced code block
            if fenced.get(i).copied().unwrap_or(false) {
                let mut code = String::new();
                let word = syntax::fence_language(line);
                let open = i;
                i += 1;
                while i < lines.len() && !(lines[i].1.trim_start().starts_with("```") || lines[i].1.trim_start().starts_with("~~~")) {
                    code.push_str(lines[i].1);
                    code.push('\n');
                    i += 1;
                }
                i += 1;
                // the box of EDIT-080: a border, and the language (or "code") in a badge at the top right
                let folded = frame.as_ref().and_then(|f| f.view.badges.binary_search_by_key(&open, |b| b.0).ok().map(|k| (f.view.badges[k].1, f.mail)));
                egui::Frame::new().fill(theme::palette().code_bg).stroke(egui::Stroke::new(1.0, theme::palette().border)).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let badge = syntax::badge_language(&word).unwrap_or_else(|| "code".to_string());
                    let box_rect = ui.max_rect();
                    let row_y = ui
                        .with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            let badge = ui.label(egui::RichText::new(badge).monospace().size((base * 0.75).max(9.0)).color(theme::palette().faint));
                            // the copy button (EDIT-093): left of the badge, while the pointer is over the box
                            let id = crate::code_box::copy_id(ui.scope_id(), rel, open);
                            let shown = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| box_rect.expand(10.0).contains(p));
                            crate::code_box::copy_button(ui, id, badge.rect.left() - 6.0, badge.rect.center().y, shown, || {
                                let all: Vec<&str> = lines.iter().map(|l| l.1).collect();
                                crate::code_box::code_of(&all, open).unwrap_or_default()
                            });
                            badge.rect.center().y
                        })
                        .inner;
                    match folded {
                        // a folded block is its first line in the box: the badge in the row, no body (EDIT-095, EDIT-098)
                        Some((n, mail)) => {
                            let g = ui.painter().layout_no_wrap(crate::fold::badge_label(FoldKind::Code, mail, n), FontId::proportional((base * 0.8).max(9.0)), theme::palette().muted);
                            let rect = egui::Rect::from_min_size(Pos2::new(box_rect.left(), row_y - (g.size().y + 2.0) / 2.0), g.size() + Vec2::new(10.0, 2.0));
                            let resp = ui.interact(rect, ui.scope_id().with(("fold-badge-in-box", rel, open)), Sense::click());
                            ui.painter().rect_filled(rect, 4.0, if resp.hovered() { theme::palette().bg_active } else { theme::palette().bg_secondary });
                            ui.painter().galley(rect.center() - g.size() / 2.0, g, theme::palette().muted);
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            in_box = Some((rect, resp.on_hover_text("Unfold").clicked()));
                        }
                        None => {
                            ui.label(code_job(&word, code.trim_end(), base * 0.9));
                        }
                    }
                });
                ui.add_space(6.0);
                break 'line;
            }
            let trimmed = line.trim();
            let indent = (line.len() - line.trim_start().len()) as f32 * 7.0;
            // embed on its own line
            if line_links.len() == 1 && line_links[0].0.embed && trimmed.len() == line_links[0].0.range.len() {
                let (l, ok) = &line_links[0];
                if *ok {
                    let shown = vault.look_media(ui.ctx(), &l.target, Some(rel));
                    let outside = matches!(shown, crate::vault::Shown::Outside(_));
                    if let Some((target, abs)) = shown.file(vault) {
                        if crate::media::is_media(&target) {
                            // a sound is a player of its own, without the frame (MEDIA-001)
                            crate::media::widget::show(ui, caches.media, &abs, rel, l.embed_width());
                            if outside {
                                outside_row(ui, &l.target, &mut out);
                            }
                            ui.add_space(6.0);
                        } else {
                            embed(ui, vault, (&target, &abs, outside.then_some(l.target.as_str())), l.embed_width(), caches, (base, dark_canvas), &mut out);
                        }
                        i += 1;
                        break 'line;
                    }
                    // a library that is not set up, a file that is not there (MEDIA-016)
                    if let Some(text) = shown.notice() {
                        embed_notice(ui, &text);
                        i += 1;
                        break 'line;
                    }
                    // the first look at the file is on its way (MEDIA-024)
                    if let Some(abs) = shown.checking() {
                        embed_loading(ui, abs);
                        i += 1;
                        break 'line;
                    }
                }
            }
            // how the paragraph or heading on this line is laid out (PDF-028): the wrapper it is in
            let align = markdown::align_at(&wrappers, i);
            if trimmed.is_empty() {
                // the blank line a wrapper has inside, next to its tag, makes no gap (EDIT-110)
                if !wrappers.iter().any(|b| b.holds(i) && (i == b.open + 1 || i + 1 == b.close)) {
                    ui.add_space(base * 0.6);
                }
            } else if markdown::is_align_edge(&wrappers, i) {
                // the tag of a wrapper is not shown (EDIT-110)
            } else if page_breaks.contains(&i) {
                // a page-break marker: a dashed rule with its label (EDIT-065)
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), base * 1.6), egui::Sense::hover());
                paint_page_break(ui.painter(), rect.left(), rect.right(), rect.center().y);
            } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            } else if let Some(def) = notes.def_at(i) {
                // a footnote definition: its label raised, its text a little smaller (EDIT-050)
                let (job, spans, _, maths) = footnote_def_painted(line, &def.label, &def.marker, &line_links, base, theme::palette().text);
                inline_block(ui, job, &spans, &maths, &hint, &mut out);
            } else if notes.is_continuation(i) {
                let (job, spans, _, maths) = footnote_text_painted(line, &line_links, base, theme::palette().text);
                ui.horizontal_top(|ui| {
                    ui.add_space(base * 1.2);
                    inline_block(ui, job, &spans, &maths, &hint, &mut out);
                });
            } else if let Some(level) = markdown::heading_level(line) {
                ui.add_space(base * 0.5);
                let size = base * heading_scale(level as u8);
                let (job, spans, maths) = inline_job(line[level..].trim_start(), &shift(&line_links, level + (line[level..].len() - line[level..].trim_start().len())), size, theme::palette().heading, false);
                let shown = ui.horizontal_top(|ui| {
                    if let Some(n) = extras.numbers.get(&i) {
                        ui.label(egui::RichText::new(n).size(size).color(theme::palette().muted));
                    }
                    inline_block_aligned(ui, job, &spans, &maths, &hint, align, &mut out)
                });
                if extras.scroll_to == Some(i) {
                    shown.response.scroll_to_me(Some(egui::Align::TOP));
                }
                ui.add_space(base * 0.2);
            } else if let Some((check, done, rest, _)) = markdown::parse_task_line(line) {
                let rest_off = line.len() - rest.len();
                ui.horizontal_top(|ui| {
                    ui.add_space(indent);
                    let t = line.trim_start();
                    let digits = t.bytes().take_while(|c| c.is_ascii_digit()).count();
                    if digits > 0 {
                        ui.label(egui::RichText::new(format!("{}.", &t[..digits])).size(base).color(theme::palette().accent));
                    }
                    let mut d = done;
                    if ui.checkbox(&mut d, "").clicked() {
                        out = Some(ReaderAction::ToggleTask(off + check));
                    }
                    let (job, spans, maths) = inline_job(rest, &shift(&line_links, rest_off), base, theme::palette().text, done);
                    inline_block(ui, job, &spans, &maths, &hint, &mut out);
                });
            } else if let Some((marker, rest_off)) = list_marker(line) {
                ui.horizontal_top(|ui| {
                    ui.add_space(indent + 4.0);
                    ui.label(egui::RichText::new(marker).size(base).color(theme::palette().accent));
                    let (job, spans, maths) = inline_job(&line[rest_off..], &shift(&line_links, rest_off), base, theme::palette().text, false);
                    inline_block(ui, job, &spans, &maths, &hint, &mut out);
                });
            } else if trimmed.starts_with('>') {
                let content_off = line.find('>').unwrap() + 1;
                let content = &line[content_off..];
                let lead = content.len() - content.trim_start().len();
                egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
                    let (job, spans, maths) = inline_job(content.trim_start(), &shift(&line_links, content_off + lead), base, theme::palette().quote, false);
                    inline_block(ui, job, &spans, &maths, &hint, &mut out);
                    let r = ui.min_rect();
                    ui.painter().line_segment([r.left_top() - Vec2::new(10.0, 0.0), r.left_bottom() - Vec2::new(10.0, 0.0)], Stroke::new(3.0, theme::palette().accent.gamma_multiply(0.7)));
                });
            } else {
                let (job, spans, maths) = inline_job(line, &line_links, base, theme::palette().text, false);
                inline_block_aligned(ui, job, &spans, &maths, &hint, align, &mut out);
            }
            i += 1;
        }
        if let (Some(f), Some(fr)) = (folds.as_mut(), frame.as_ref()) {
            fold_widgets(ui, f.ui, rel, fr, (start, top), base, in_box);
        }
        if let Some(h) = &heights {
            h.lock().unwrap_or_else(|e| e.into_inner()).segs.insert(start, (i, ui.cursor().min.y - top));
        }
    }
    out
}

/// From this many lines on, the reading view does not draw what is far from the visible part (EDIT-102).
const CULL_LINES: usize = 2000;

/// What the heights of a note's parts were drawn for: they are valid for the same text, width, size and folds.
#[derive(Clone, PartialEq)]
struct HeightsKey {
    text: u64,
    width: u32,
    base: u32,
    badges: Vec<(usize, usize)>,
}

/// The height of each part of the reading view (a line, a table, a code block...) by the line it starts at,
/// and the line after it.
#[derive(Default)]
struct Heights {
    key: Option<HeightsKey>,
    segs: HashMap<usize, (usize, f32)>,
}

/// The marker and the badge of the foldable blocks that start at `line` of the reading view (EDIT-096), drawn
/// after the line: `at` is the line and `top`, where it began. The front matter is the properties card, with its own toggle
/// (EDIT-063), and has none. A folded code block drew its badge in its box (`in_box`, EDIT-098).
fn fold_widgets(ui: &mut Ui, folds: &mut FoldUi, rel: &Path, frame: &Frame, at: (usize, f32), base: f32, in_box: Option<(egui::Rect, bool)>) {
    let (line, top) = at;
    // the blocks are in the order of their headers
    let first = frame.blocks.partition_point(|b| b.header < line);
    let here: Vec<usize> = (first..frame.blocks.len()).take_while(|&i| frame.blocks[i].header == line).filter(|&i| frame.blocks[i].kind != FoldKind::Front).collect();
    if here.is_empty() {
        return;
    }
    let col = ui.max_rect();
    let bottom = ui.cursor().min.y;
    let folded = here.iter().any(|i| frame.folded.binary_search(i).is_ok());
    let hover = ui.input(|i| i.pointer.hover_pos());
    let hot = hover.is_some_and(|p| p.y >= top && p.y <= bottom && p.x >= col.left() - 24.0 && p.x <= col.right());
    let mut clicked = None;
    let mut widget = Widget { line, marker: None, badge: None, copy: None };
    let clip = ui.clip_rect();
    if (folded || hot) && bottom >= clip.min.y && top <= clip.max.y {
        let first = &frame.blocks[here[0]];
        let y = match (first.kind, markdown::heading_level(&first.title)) {
            (FoldKind::Heading, Some(level)) => top + base * 0.5 + base * heading_scale(level as u8) * 0.65,
            (FoldKind::Code, _) => top + 10.0 + base * 0.55,
            _ => top + base * 0.7,
        };
        let rect = egui::Rect::from_center_size(Pos2::new(col.left() - crate::fold_ui::MARKER / 2.0 - 4.0, y), Vec2::splat(crate::fold_ui::MARKER));
        if crate::fold_ui::marker_button(ui, ui.scope_id().with(("reader-fold-marker", rel, line)), rect, folded) {
            clicked = Some(FoldCmd::Toggle);
        }
        widget.marker = Some(rect);
    }
    if let Some(&(_, n)) = frame.view.badges.binary_search_by_key(&line, |b| b.0).ok().map(|i| &frame.view.badges[i]) {
        let (rect, clicked_badge) = match in_box {
            Some(drawn) => drawn,
            None => {
                let kind = frame.folded.iter().map(|&i| &frame.blocks[i]).filter(|b| b.header == line).max_by_key(|b| b.end).map_or(FoldKind::Heading, |b| b.kind);
                let g = ui.painter().layout_no_wrap(crate::fold::badge_label(kind, frame.mail, n), FontId::proportional((base * 0.8).max(9.0)), theme::palette().muted);
                let (rect, resp) = ui.allocate_exact_size(g.size() + Vec2::new(10.0, 2.0), Sense::click());
                ui.painter().rect_filled(rect, 4.0, if resp.hovered() { theme::palette().bg_active } else { theme::palette().bg_secondary });
                ui.painter().galley(rect.center() - g.size() / 2.0, g, theme::palette().muted);
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                (rect, resp.on_hover_text("Unfold").clicked())
            }
        };
        if clicked_badge {
            clicked = Some(FoldCmd::Unfold);
        }
        widget.badge = Some(rect);
    }
    folds.widgets.push(widget);
    if let Some(cmd) = clicked {
        folds.apply(rel, frame, cmd, line);
        ui.ctx().request_repaint();
    }
}

/// A table drawn as a grid (EDIT-024); links in its cells work as everywhere in the reading view.
/// The table's comment line (EDIT-030) is not shown; its sizes (EDIT-031) are used.
fn table_block(ui: &mut Ui, b: &crate::table::Block, vault: &Vault, rel: &Path, base: f32, hint: &dyn Fn(&str) -> Option<String>, out: &mut Option<ReaderAction>) {
    let laid = crate::table::layout(ui.ctx(), &b.table, &b.meta.sizes, base, ui.available_width(), &|l| vault.exists(&l.target, Some(rel)));
    let (rect, resp) = ui.allocate_exact_size(laid.size(), Sense::click());
    crate::table::paint(ui.painter(), rect.min, &laid);
    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
        if let Some((r, c, anchor)) = laid.cell_at(rect.min, hp) {
            let cell = &laid.cells[r][c];
            if let Some(ch) = char_under(&cell.galley, anchor, hp) {
                let byte = markdown::char_to_byte(&cell.galley.job.text, ch);
                // a formula of the cell that failed says why (EDIT-049)
                if let Some(Err(why)) = cell.maths.iter().find(|m| m.range.contains(&byte)).map(|m| &m.formula) {
                    resp.clone().on_hover_text(format!("This formula cannot be typeset: {why}"));
                }
                follow_span(ui, &resp, byte, &cell.spans, hint, out);
            }
        }
    }
    ui.add_space(base * 0.4);
}

fn shift(links: &[(LinkSpan, bool)], by: usize) -> Vec<(LinkSpan, bool)> {
    links
        .iter()
        .filter(|(l, _)| l.range.start >= by)
        .map(|(l, ok)| {
            let mut l = l.clone();
            l.range = l.range.start - by..l.range.end - by;
            (l, *ok)
        })
        .collect()
}

fn list_marker(line: &str) -> Option<(String, usize)> {
    let t = line.trim_start();
    let ind = line.len() - t.len();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return Some(("•".into(), ind + 2));
    }
    let digits = t.bytes().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return Some((format!("{}.", &t[..digits]), ind + digits + 2));
    }
    None
}

/// The line under the player of an outside file: it says the file is not synced (MEDIA-023) and
/// has "Copy into the vault" (MEDIA-021).
fn outside_row(ui: &mut Ui, target: &str, out: &mut Option<ReaderAction>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::media::outside::NOT_SYNCED).color(theme::palette().muted).small());
        if ui.small_button(crate::media::outside::COPY_LABEL).on_hover_text(COPY_HINT).clicked() {
            *out = Some(ReaderAction::CopyOutside(target.to_string()));
        }
    });
}

const COPY_HINT: &str = "Copy the file into the vault, in the media folder, and link the copy. Files outside the vault are not synced.";

/// The strip that stands in place of an outside file that cannot be shown (MEDIA-016).
fn embed_notice(ui: &mut Ui, text: &str) {
    egui::Frame::new().fill(theme::palette().bg_secondary).stroke(Stroke::new(1.0, theme::palette().border)).corner_radius(CornerRadius::same(8)).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        crate::media::widget::show_notice(ui, text);
    });
    ui.add_space(6.0);
}

/// The quiet strip of an embed whose file is looked up or whose picture is decoded on a worker (MEDIA-024);
/// as high as the strip of an outside file that is not found, so the page hardly moves.
fn embed_loading(ui: &mut Ui, abs: &Path) {
    egui::Frame::new().fill(theme::palette().bg_secondary).stroke(Stroke::new(1.0, theme::palette().border)).corner_radius(CornerRadius::same(8)).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        crate::media::widget::show_loading(ui, &abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    });
    ui.add_space(6.0);
}

/// `width` is what the link asks for (`![[pic.png|300]]`, EDIT-052); `text` is the text size and
/// whether drawings get a dark canvas.
/// `file` is the file's path (in the vault, or the absolute path of an outside file), where it is, and for an
/// outside file the link's target.
fn embed(ui: &mut Ui, vault: &Vault, (target, abs, outside): (&Path, &Path, Option<&str>), width: Option<f32>, caches: &mut Caches, text: (f32, bool), out: &mut Option<ReaderAction>) {
    let (base, dark) = text;
    // a click opens the file: a file of the vault in its program or tab, an outside one through its link (MEDIA-017)
    let open = || match outside {
        Some(link) => ReaderAction::OpenLink { target: link.to_string(), heading: None, new_tab: false },
        None => ReaderAction::OpenFile(target.to_path_buf()),
    };
    let kind = vault.file(target).map(|f| f.kind).unwrap_or(FileKind::Other);
    let name = vault.file(target).map(|f| f.display_name()).unwrap_or_else(|| abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let frame = egui::Frame::new().fill(theme::palette().bg_secondary).stroke(Stroke::new(1.0, theme::palette().border)).corner_radius(CornerRadius::same(8)).inner_margin(8);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let label = ui.label(egui::RichText::new(&name).color(theme::palette().muted).small());
            if outside.is_some() {
                label.on_hover_text(crate::media::outside::NOT_SYNCED);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Open").clicked() {
                    *out = Some(open());
                }
                if let Some(link) = outside {
                    if ui.small_button(crate::media::outside::COPY_LABEL).on_hover_text(COPY_HINT).clicked() {
                        *out = Some(ReaderAction::CopyOutside(link.to_string()));
                    }
                }
            });
        });
        match kind {
            FileKind::Drawing => {
                let h = 240.0;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
                let bg = if dark { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE };
                ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
                if let Some(scene) = caches.scenes.get(vault, target) {
                    let pics = crate::drawing::pictures::textures(ui.ctx(), scene, vault, target);
                    drawing::paint_thumbnail(ui.painter(), scene, rect.shrink(6.0), dark, &pics);
                }
                if resp.on_hover_text("Open drawing").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    *out = Some(open());
                }
            }
            FileKind::Note => {
                let text = vault.notes.get(target).map(|n| n.text.clone()).unwrap_or_default();
                let preview: String = text.lines().take(12).collect::<Vec<_>>().join("\n");
                ui.label(egui::RichText::new(preview).size(base * 0.95).color(theme::palette().text));
            }
            FileKind::Other if crate::pdf::view::is_pdf(target) => {
                let w = ui.available_width();
                match crate::pdf::view::pdf_pages(ui.ctx(), abs, 50) {
                    crate::pdf::view::PdfState::Ready { pages, total } => {
                        for (i, tex) in pages.iter().enumerate() {
                            let [tw, th] = tex.size_vec2().into();
                            let k = (w / tw).min(1.0);
                            let resp = ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), egui::vec2(tw * k, th * k))).sense(Sense::click()));
                            if resp.on_hover_text(format!("Page {} of {total} · click to open in your PDF viewer", i + 1)).clicked() {
                                *out = Some(open());
                            }
                            ui.add_space(4.0);
                        }
                        if total > pages.len() {
                            ui.label(egui::RichText::new(format!("{} more pages — open the PDF to see them all", total - pages.len())).color(theme::palette().muted));
                        }
                    }
                    crate::pdf::view::PdfState::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Rendering PDF…").color(theme::palette().muted));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    crate::pdf::view::PdfState::Failed(e) => {
                        ui.label(egui::RichText::new(format!("This PDF can't be shown: {e}")).color(theme::palette().muted));
                    }
                }
            }
            FileKind::Other if crate::media::is_image(target) => {
                let w = ui.available_width();
                // still a Git LFS pointer: "Not downloaded (12 MB) — Download" (SYNC-020)
                if crate::media::widget::show_not_downloaded(ui, caches.media, abs).is_some() {
                    ui.add_space(6.0);
                    return;
                }
                match crate::media::picture(ui.ctx(), abs) {
                    crate::media::look::Pic::Ready(tex) => {
                        let img = match width {
                            Some(asked) => crate::media::fitted(&tex, asked.min(w), f32::INFINITY),
                            None => crate::media::fitted(&tex, w, 520.0),
                        };
                        let resp = ui.add(img.corner_radius(CornerRadius::same(4)).sense(Sense::click()));
                        if resp.on_hover_text("Open with your image viewer").clicked() {
                            *out = Some(open());
                        }
                    }
                    crate::media::look::Pic::Loading => {
                        crate::media::widget::show_loading(ui, &name);
                    }
                    crate::media::look::Pic::Broken => {
                        ui.label(egui::RichText::new("This picture can't be shown (unsupported or damaged file).").color(theme::palette().muted));
                    }
                }
            }
            FileKind::Other => {
                ui.label(egui::RichText::new("This kind of file can't be shown here yet.").color(theme::palette().muted));
            }
        }
    });
    ui.add_space(6.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};

    // spec: EDIT-032
    #[test]
    fn the_reading_view_colours_code_blocks_by_language_and_inline_code() {
        let colours = |job: &LayoutJob, needle: &str| {
            let at = job.text.find(needle).unwrap();
            job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format.color
        };
        let code = "console.log(\"foooo\")";
        let job = code_job("js", code, 14.0);
        assert_eq!(job.text, code, "the text is the code, nothing else");
        assert_eq!(colours(&job, "\"foooo\""), theme::palette().syntax(syntax::Kind::String));
        assert_ne!(colours(&job, "\"foooo\""), colours(&job, "console"));
        let plain = code_job("klingon", code, 14.0);
        assert_eq!((plain.text.as_str(), colours(&plain, "\"foooo\"")), (code, theme::palette().code), "an unknown language is plain monospace");
        assert_eq!(code_job("", "", 14.0).text, "", "an empty block");

        let line = "see `console.log(\"fooo\")` and `x`";
        let (job, _, _) = inline_job(line, &[], 14.0, theme::palette().text, false);
        assert_eq!(job.text, "see console.log(\"fooo\") and x", "the backticks are gone");
        assert_eq!(colours(&job, "\"fooo\""), theme::palette().syntax(syntax::Kind::String));
        assert_eq!(colours(&job, "log"), theme::palette().syntax(syntax::Kind::Function));
        assert_eq!(colours(&job, "x"), theme::palette().code);
    }

    fn shapes_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    // spec: EDIT-024, EDIT-030, EDIT-031
    #[test]
    fn the_reading_view_draws_a_table_as_a_grid_and_its_links_work() {
        let d = std::env::temp_dir().join(format!("shale-reader-table-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        // the table's comment line (EDIT-030) is not shown
        let text = "before\n\n<!-- shale-table edit=raw cols=200,90 -->\n| Name | Note |\n|---|:-:|\n| `a|b` | [[Ideas]] |\n\nafter";
        let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |events: Vec<Event>| {
            time += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
            let mut action = None;
            let mut out = ctx.run_ui(raw, |ui| {
                action = show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
            });
            out.textures_delta.clear();
            (action, shapes_text(&out.shapes))
        };
        frame(vec![]);
        let (_, painted) = frame(vec![]);
        let names: Vec<&str> = painted.iter().map(|(t, _)| t.as_str()).collect();
        for cell in ["Name", "Note", "a|b", "Ideas", "before", "after"] {
            assert!(names.contains(&cell), "{cell} is painted on its own: {names:?}");
        }
        assert!(!names.iter().any(|t| t.contains('|') && t.len() > 3), "no raw table text: {names:?}");
        assert!(!names.iter().any(|t| t.contains("shale-table")), "no comment line: {names:?}");
        // its sizes are used (EDIT-031): the first column is 200 points wide
        let (name, note) = (painted.iter().find(|(t, _)| t == "Name").unwrap().1, painted.iter().find(|(t, _)| t == "Note").unwrap().1);
        assert!(note.center().x - name.left() > 200.0, "{name:?} {note:?}");
        let at = painted.iter().find(|(t, _)| t == "Ideas").unwrap().1.center();
        let click = |pressed| vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }];
        frame(click(true));
        match frame(click(false)).0 {
            Some(ReaderAction::OpenLink { target, .. }) => assert_eq!(target, "Ideas"),
            _ => panic!("clicking the link in the table did nothing"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Draw `text` in the reading view of a 700 x 500 window (in a scroll area, like the app's)
    /// for some frames (a scroll is animated); returns the last action and what was painted.
    fn read(v: &Vault, text: &str, extras: &Extras, frames: Vec<Vec<Event>>) -> (Option<ReaderAction>, Vec<(String, Rect)>) {
        let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut last = (None, vec![]);
        for (n, events) in std::iter::repeat_with(Vec::new).take(3).chain(frames).chain(std::iter::repeat_with(Vec::new).take(20)).enumerate() {
            time += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
            let mut action = None;
            let mut out = ctx.run_ui(raw, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // the target is asked for in the first frame only, as the app does
                    let e = Extras { numbers: extras.numbers.clone(), scroll_to: extras.scroll_to.filter(|_| n == 0), properties: extras.properties, folds: None };
                    action = show(ui, text, v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &e);
                });
            });
            out.textures_delta.clear();
            last = (action.or(last.0), shapes_text(&out.shapes));
        }
        last
    }

    // spec: LINK-014
    #[test]
    fn the_reading_view_draws_the_numbers_of_the_headings_before_their_text() {
        let d = std::env::temp_dir().join(format!("shale-reader-numbers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        let text = "# Title\n## Plan\n### Step\n## Next\n";
        let (_, painted) = read(&v, text, &Extras { numbers: Extras::numbers_of(text, true), scroll_to: None, ..Default::default() }, vec![vec![]]);
        for (n, title) in [("1", "Title"), ("1.1", "Plan"), ("1.1.1", "Step"), ("1.2", "Next")] {
            let number = painted.iter().find(|(t, _)| t == n).unwrap_or_else(|| panic!("{n} is painted: {painted:?}")).1;
            let title = painted.iter().find(|(t, _)| t == title).unwrap().1;
            assert!(number.max.x <= title.min.x + 0.5 && (number.center().y - title.center().y).abs() < 6.0, "{n} is left of {title:?} on its row: {number:?}");
        }
        assert!(!painted.iter().any(|(t, _)| t == "0"), "no heading is numbered 0");
        let (_, painted) = read(&v, text, &Extras { numbers: Extras::numbers_of(text, false), scroll_to: None, ..Default::default() }, vec![vec![]]);
        assert!(!painted.iter().any(|(t, _)| t == "1" || t == "1.1"), "numbering off: {painted:?}");
        let on = "---\nnumbered-headings: yes\n---\n## Plan\n";
        assert_eq!(Extras::numbers_of(on, false).get(&3).map(String::as_str), Some("1"), "front matter turns it on (line 3, after the properties)");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-015
    #[test]
    fn a_heading_link_in_the_reading_view_carries_its_heading_and_the_view_scrolls_to_a_heading() {
        let d = std::env::temp_dir().join(format!("shale-reader-heading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        for (link, shown) in [("[[Ideas#Top part]]", "Ideas > Top part"), ("[to](Ideas.md#Top%20part)", "to")] {
            let (_, painted) = read(&v, link, &Extras::default(), vec![vec![]]);
            let at = painted.iter().find(|(t, _)| t == shown).unwrap_or_else(|| panic!("{shown} is painted: {painted:?}")).1.center();
            let click = |pressed| vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }];
            match read(&v, link, &Extras::default(), vec![click(true), click(false)]).0 {
                Some(ReaderAction::OpenLink { target, heading, .. }) => assert_eq!((target.as_str(), heading.as_deref()), ("Ideas", Some("Top part")), "{link}"),
                _ => panic!("clicking {link} did nothing"),
            }
        }
        // the heading far down is at the top of the view after the scroll
        let text: String = (0..80).map(|i| format!("line {i}\n")).collect::<String>() + "## Down here\n" + &"after\n".repeat(60);
        let line = text.lines().position(|l| l == "## Down here").unwrap();
        let (_, before) = read(&v, &text, &Extras::default(), vec![vec![]]);
        assert!(before.iter().find(|(t, _)| t == "Down here").unwrap().1.min.y > 1000.0, "far below the window");
        let (_, after) = read(&v, &text, &Extras { numbers: Default::default(), scroll_to: Some(line), ..Default::default() }, vec![vec![]]);
        let y = after.iter().find(|(t, _)| t == "Down here").unwrap().1.min.y;
        assert!((0.0..40.0).contains(&y), "the heading is at the top of the view: {y}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-029
    #[test]
    fn the_reading_view_colours_the_text_and_hides_the_tags() {
        let line = "a <mark style=\"background: #FFFF00;\">hi <span style=\"color: #FF0000;\">there</span></mark> **b** <mark style=\"background: #FFFF00\">open";
        let (job, _, paints, _) = inline_painted(line, &[], 15.0, theme::palette().text, false);
        assert_eq!(job.text, "a hi there b <mark style=\"background: #FFFF00\">open", "the tags are gone; an unclosed one is text");
        let at = |w: &str| {
            let b = job.text.find(w).unwrap();
            job.sections.iter().find(|s| s.byte_range.start.0 <= b && b < s.byte_range.end.0).unwrap().format.clone()
        };
        let yellow = Color32::from_rgb(255, 255, 0);
        assert_eq!((at("hi").background, at("hi").color), (yellow, Color32::BLACK));
        assert_eq!((at("there").background, at("there").color), (yellow, Color32::from_rgb(255, 0, 0)));
        assert_eq!(at("b ").background, Color32::TRANSPARENT);
        assert!(at("b ").extra_letter_spacing > 0.0, "bold after the mark is still bold");
        // the paints say where the colours are, for the PDF
        let hi = job.text.find("hi ").unwrap();
        let there = job.text.find("there").unwrap();
        assert_eq!(paints, vec![(hi..there, Some([255, 255, 0, 255]), None), (there..there + 5, Some([255, 255, 0, 255]), Some([255, 0, 0, 255]))]);
    }

    fn format_at(job: &LayoutJob, word: &str) -> TextFormat {
        let b = job.text.find(word).unwrap_or_else(|| panic!("{word} is not in {:?}", job.text));
        job.sections.iter().find(|s| s.byte_range.start.0 <= b && b < s.byte_range.end.0).unwrap().format.clone()
    }

    // spec: EDIT-051
    #[test]
    fn the_reading_view_underlines_lowers_and_raises_and_hides_the_tags() {
        let line = "Plain <u>under</u> H<sub>2</sub>O x<sup>2</sup> <U>a <SUP>b</SUP></U> <u>open `<sub>x</sub>`";
        let (job, _, paints, _) = inline_painted(line, &[], 15.0, theme::palette().text, false);
        assert_eq!(job.text, "Plain under H2O x2 a b <u>open <sub>x</sub>", "the tags are gone; an unclosed one and one in code are text");
        assert_eq!(format_at(&job, "under").underline.width, 1.0);
        assert_eq!(format_at(&job, "Plain").underline.width, 0.0);
        let (sub, sup) = (format_at(&job, "2O"), format_at(&job, "2 a"));
        assert_eq!((sub.font_id.size, sub.valign, sub.line_height), (15.0 * SCRIPT_SCALE, egui::Align::BOTTOM, None));
        assert_eq!((sup.font_id.size, sup.valign, sup.line_height), (15.0 * SCRIPT_SCALE, egui::Align::TOP, None));
        assert_eq!(format_at(&job, "O x").font_id.size, 15.0);
        let b = format_at(&job, "b <u>");
        assert_eq!((b.underline.width, b.valign), (1.0, egui::Align::TOP), "nested: underlined and raised");
        assert_eq!(format_at(&job, "<u>open").underline.width, 0.0);
        assert!(paints.is_empty(), "they are no colours");
    }

    // spec: EDIT-050
    #[test]
    fn a_reference_with_a_definition_is_a_raised_mark_and_one_without_stays_as_written() {
        let line = "Note[^1] and[^2] and `[^1]`";
        let _defined = markdown::DefinedFootnotes::set(["1".to_string()].into());
        let (job, spans, _, _) = inline_painted(line, &[], 15.0, theme::palette().text, false);
        assert_eq!(job.text, "Note1 and[^2] and [^1]");
        let mark = format_at(&job, "1 and");
        assert_eq!((mark.valign, mark.font_id.size, mark.color, mark.line_height), (egui::Align::TOP, 15.0 * SCRIPT_SCALE, theme::palette().link, None));
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].0.clone(), footnote_of(&spans[0].1)), (4..5, Some((Foot::Reference, "1"))));
        assert_eq!(format_at(&job, "[^2]").valign, egui::Align::BOTTOM);
        // the same line without any definition
        drop(_defined);
        let (job, spans, _, _) = inline_painted(line, &[], 15.0, theme::palette().text, false);
        assert_eq!(job.text, "Note[^1] and[^2] and [^1]");
        assert!(spans.is_empty());
    }

    // spec: EDIT-050
    #[test]
    fn a_definition_shows_its_label_raised_and_its_text_smaller_with_its_links_working() {
        let line = "[^a]:   See **bold** [[Ideas]] and[^a]";
        let notes = markdown::find_footnotes(line);
        let _defined = markdown::DefinedFootnotes::set(notes.defined());
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(line).into_iter().map(|l| (l, true)).collect();
        let def = &notes.defs[0];
        let (job, spans, _, _) = footnote_def_painted(line, &def.label, &def.marker, &links, 15.0, theme::palette().text);
        assert_eq!(job.text, "a See bold Ideas anda");
        let label = format_at(&job, "a See");
        assert_eq!((label.valign, label.color, label.font_id.size), (egui::Align::TOP, theme::palette().link, 15.0 * FOOTNOTE_TEXT * SCRIPT_SCALE));
        assert_eq!(format_at(&job, "See").font_id.size, 15.0 * FOOTNOTE_TEXT);
        assert_eq!(format_at(&job, "bold").color, theme::palette().text_strong);
        let kinds: Vec<(Range<usize>, Option<Foot>, String)> = spans.iter().map(|(r, l, _)| (r.clone(), footnote_of(l).map(|f| f.0), l.target.clone())).collect();
        assert_eq!(kinds, [(0..1, Some(Foot::Definition), "a".to_string()), (11..16, None, "Ideas".to_string()), (20..21, Some(Foot::Reference), "a".to_string())]);
        // an indented line that goes on with it is smaller text
        let (job, _, _, _) = footnote_text_painted("    more *text*", &[], 15.0, theme::palette().text);
        assert_eq!(job.text, "more text");
        assert_eq!(format_at(&job, "more").font_id.size, 15.0 * FOOTNOTE_TEXT);
    }

    // spec: EDIT-045
    #[test]
    fn br_in_a_table_cell_is_a_line_break_and_elsewhere_it_is_text() {
        for tag in ["<br>", "<br/>", "<br />", "<BR>", "<Br  / >"] {
            let line = format!("• a{tag}• b");
            let (job, _, _, _) = cell_painted(&line, &[], 15.0, theme::palette().text);
            let want = if tag == "<Br  / >" { line.clone() } else { "• a\n• b".to_string() };
            assert_eq!(job.text, want, "{tag}");
        }
        let (job, _, _, _) = cell_painted("a `x<br>y` b<br>", &[], 15.0, theme::palette().text);
        assert_eq!(job.text, "a x<br>y b\n", "not in code");
        let (job, _, _, _) = inline_painted("a<br>b", &[], 15.0, theme::palette().text, false);
        assert_eq!(job.text, "a<br>b", "outside a table the tag is text");
        assert_eq!(br_len("<br>x"), Some(4));
        assert_eq!(br_len("<br />"), Some(6));
        assert_eq!(br_len("<b>"), None);
        assert_eq!(br_len("<bro>"), None);
        assert_eq!(br_len("<br"), None);
        assert_eq!(br_len("é<br>"), None);
    }

    // spec: EDIT-050
    #[test]
    fn a_click_on_a_footnote_mark_scrolls_to_its_definition_and_back() {
        let d = std::env::temp_dir().join(format!("shale-reader-footnote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        let text = "Claim[^1] here.\n\n".to_string() + &"filler\n".repeat(80) + "\n[^1]: The body\n    and more\n\n" + &"after\n".repeat(40);
        let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |events: Vec<Event>, n: usize| {
            let mut last = vec![];
            for k in 0..n {
                time += 0.05;
                let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), events: if k == 0 { events.clone() } else { vec![] }, ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        show(ui, &text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
                    });
                });
                out.textures_delta.clear();
                last = shapes_text(&out.shapes);
            }
            last
        };
        let click = |at: egui::Pos2, pressed| vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }];
        let find = |p: &[(String, Rect)], t: &str| p.iter().find(|(s, _)| s == t).map(|(_, r)| *r);
        frame(vec![], 3);
        let before = frame(vec![], 1);
        assert!(!before.iter().any(|(t, _)| t.contains("[^")), "no raw footnote markup: {before:?}");
        assert!(find(&before, "1 The body").is_none_or(|r| r.min.y > 1000.0), "the definition is far below the window");
        // the mark is a part of the text of its paragraph: the middle of its character
        let claim = find(&before, "Claim1 here.").unwrap_or_else(|| panic!("the paragraph is painted: {before:?}"));
        let mark = {
            let _defined = markdown::DefinedFootnotes::set(["1".to_string()].into());
            let (job, _, _, _) = inline_painted("Claim[^1] here.", &[], 15.0, theme::palette().text, false);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            claim.min + galley.pos_from_cursor(egui::text::CCursor::new(5)).center().to_vec2() + egui::Vec2::new(2.0, 0.0)
        };
        frame(click(mark, true), 1);
        let after = frame(click(mark, false), 25);
        let body = find(&after, "1 The body").unwrap_or_else(|| panic!("the definition is painted after the jump: {after:?}"));
        assert!((0.0..60.0).contains(&body.min.y), "the definition is at the top of the view: {body:?}");
        assert!(find(&after, "and more").is_some(), "its continuation line is shown: {after:?}");
        // the label of the definition goes back to the reference
        let label = body.min + egui::Vec2::new(2.0, body.height() / 2.0);
        frame(click(label, true), 1);
        let back = frame(click(label, false), 25);
        let claim = find(&back, "Claim1 here.").unwrap_or_else(|| panic!("the reference is painted again: {back:?}"));
        assert!((0.0..60.0).contains(&claim.min.y), "the reference is at the top of the view again: {claim:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: CFG-039
    #[test]
    fn cfg_039_the_reading_view_draws_bold_and_italic_in_the_font_s_own_faces() {
        use crate::fonts::{AreaFont, Faces};
        let fam = |n: &str| FontFamily::Name(n.into());
        let line = "**bold** *it* `code`";
        let at = |job: &LayoutJob, w: &str| {
            let b = job.text.find(w).unwrap();
            job.sections.iter().find(|s| s.byte_range.start.0 <= b && b < s.byte_range.end.0).unwrap().format.clone()
        };
        let font = AreaFont { family: fam("system:X"), size: 15.0, faces: Faces { bold: true, italic: true, bold_italic: false } };
        let (job, _, _) = crate::fonts::with_family(&font, || inline_job(line, &[], 15.0, theme::palette().text, false));
        assert_eq!((at(&job, "bold").font_id.family, at(&job, "bold").extra_letter_spacing), (fam("system:X:bold"), 0.0));
        assert_eq!((at(&job, "it").font_id.family, at(&job, "it").italics), (fam("system:X:italic"), false));
        assert_eq!(at(&job, "code").font_id.family, FontFamily::Monospace);
        // Shale's own font has no faces: bold and italic are imitated as before
        let (job, _, _) = inline_job(line, &[], 15.0, theme::palette().text, false);
        assert_eq!(at(&job, "bold").font_id.family, FontFamily::Proportional);
        assert!(at(&job, "bold").extra_letter_spacing > 0.0 && at(&job, "it").italics);
    }

    fn icon_scope(max_width: f32) -> InlinePictures {
        InlinePictures::set([("icon.png".to_string(), InlinePicture { path: PathBuf::from("/vault/icon.png"), size: Vec2::new(40.0, 30.0), texture: None })].into(), max_width)
    }

    fn embed_links(line: &str) -> Vec<(LinkSpan, bool)> {
        markdown::find_links(line).into_iter().map(|l| (l, true)).collect()
    }

    // spec: EDIT-114
    #[test]
    fn edit_114_inline_pictures_hide_the_link_text_and_keep_room_for_the_picture() {
        let line = "an icon ![[icon.png]] here";
        let links = embed_links(line);
        let _scope = icon_scope(500.0);
        let (job, spans, _, _) = inline_painted(line, &links, 14.0, theme::palette().text, false);
        let hidden: Vec<_> = job.sections.iter().filter(|s| crate::math::paint::is_hidden_formula(&s.format)).collect();
        assert_eq!(hidden.len(), 2, "the link text is two hidden sections: {:?}", job.sections);
        let text = |s: &egui::text::LayoutSection| &job.text[s.byte_range.start.0..s.byte_range.end.0];
        assert_eq!((text(hidden[0]), text(hidden[1])), ("i", "con.png"));
        // the room is the picture's width at 0.9 of the line (1.5 text sizes) of 14 points: 40 x 30 px
        let room = 14.0 * 1.5 * PICTURE_HEIGHT * 40.0 / 30.0;
        assert_eq!((hidden[0].leading_space, hidden[1].leading_space), (0.0, room));
        assert!(hidden.iter().all(|s| s.format.line_height == Some(21.0)), "the row is no taller for it");
        assert_eq!(&job.text[spans[0].0.clone()], "icon.png", "the link still has its span");
        assert!(spans[0].1.embed);
        // the other text is as it was
        assert!(job.text.starts_with("an icon ") && job.text.ends_with(" here"));
        // a link that is no embed, and an embed of something else, are link text
        let (job, ..) = inline_painted("a [[icon.png]] b ![[other.png]] c", &embed_links("a [[icon.png]] b ![[other.png]] c"), 14.0, theme::palette().text, false);
        assert!(job.sections.iter().all(|s| !crate::math::paint::is_hidden_formula(&s.format) && s.leading_space == 0.0));
    }

    // spec: EDIT-114
    #[test]
    fn edit_114_a_wide_picture_is_scaled_down_to_the_width_and_a_short_name_keeps_its_room() {
        let wide = |max| {
            let _scope = InlinePictures::set([("w.png".to_string(), InlinePicture { path: PathBuf::new(), size: Vec2::new(1000.0, 10.0), texture: None })].into(), max);
            let line = "x ![[w.png|q]] y";
            let (job, ..) = inline_painted(line, &embed_links(line), 14.0, theme::palette().text, false);
            let hidden: Vec<_> = job.sections.iter().filter(|s| crate::math::paint::is_hidden_formula(&s.format)).map(|s| (job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(), s.leading_space)).collect();
            hidden
        };
        assert_eq!(wide(80.0), [("q".to_string(), 0.0), ("_".to_string(), 80.0)], "never wider than the line; one character is padded to two");
        let size = inline_picture_size(Vec2::new(1000.0, 10.0), 21.0, 80.0);
        assert!((size.x / size.y - 100.0).abs() < 0.01 && size.x == 80.0, "proportions kept: {size:?}");
        assert_eq!(inline_picture_size(Vec2::new(30.0, 30.0), 20.0, 500.0), Vec2::splat(18.0), "0.9 of the line, square");
        let blank = {
            let _scope = icon_scope(500.0);
            let line = "x ![[icon.png|a b]] y";
            let (job, ..) = inline_painted(line, &embed_links(line), 14.0, theme::palette().text, false);
            job.sections.iter().filter(|s| crate::math::paint::is_hidden_formula(&s.format)).map(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].to_string()).collect::<Vec<_>>()
        };
        assert_eq!(blank, ["a", "_b"], "a blank is no place to break the line");
    }

    // spec: EDIT-114
    #[test]
    fn edit_114_without_a_scope_an_embed_in_a_line_is_its_link_text() {
        let line = "an icon ![[icon.png]] here";
        let links = embed_links(line);
        let plain = |links: &[(LinkSpan, bool)]| {
            let (job, spans, ..) = inline_painted(line, links, 14.0, theme::palette().text, false);
            assert!(job.text.contains("icon.png") && job.sections.iter().all(|s| !crate::math::paint::is_hidden_formula(&s.format) && s.leading_space == 0.0), "link text, no room: {:?}", job.sections);
            assert_eq!(spans.len(), 1);
        };
        plain(&links);
        // the scope ends with its guard, and the one before it comes back
        {
            let _outer = icon_scope(500.0);
            assert!(inline_picture("icon.png").is_some());
            {
                let _inner = InlinePictures::set(HashMap::new(), 10.0);
                assert!(inline_picture("icon.png").is_none());
            }
            assert_eq!(inline_picture("icon.png").map(|(_, max)| max), Some(500.0));
        }
        assert!(inline_picture("icon.png").is_none());
        plain(&links);
    }

    // spec: EDIT-114
    #[test]
    fn edit_114_an_embed_alone_on_its_line_is_no_picture_in_a_line() {
        let t = "![[a.png]]\n\ntext ![[a.png]]\n\n - ![[b.png]]\n  ![[c.png|x]]  \n";
        let links = markdown::find_links(t);
        let in_line: Vec<bool> = links.iter().map(|l| embed_is_in_a_line(t, &l.range)).collect();
        assert_eq!(in_line, [false, true, true, false]);
    }
}
