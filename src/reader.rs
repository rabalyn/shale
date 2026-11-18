//! Reading view: renders a note without markdown syntax. Links are clickable (blue = exists,
//! red = missing), checkboxes toggle, and `![[drawing.excalidraw]]` embeds show the drawing.

use crate::colour_tags;
use crate::drawing::{self, Scene};
use crate::editor::{char_under, heading_scale, FOOTNOTE_TEXT, SCRIPT_SCALE};
use crate::markdown::{self, LinkSpan};
use crate::syntax;
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Sense, Stroke, TextFormat, Ui, Vec2};
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
}

/// What the reading view shows besides the text: the numbers of the headings by line (LINK-014)
/// and the line of a heading to scroll to (LINK-015).
#[derive(Default)]
pub struct Extras {
    pub numbers: HashMap<usize, String>,
    pub scroll_to: Option<usize>,
    /// how the front matter is drawn (CFG-050)
    pub properties: crate::config::PropertiesView,
}

impl Extras {
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
        job.append(&code[at..r.start], 0.0, format(theme::CODE));
        job.append(&code[r.clone()], 0.0, format(theme::syntax_colour(*kind)));
        at = r.end;
    }
    job.append(&code[at..], 0.0, format(theme::CODE));
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
                theme::CODE
            } else if st.bold {
                theme::TEXT_STRONG
            } else {
                color
            },
            italics: if st.code { st.italic } else { styled.fake_italic },
            line_height: Some(size * 1.5),
            ..Default::default()
        };
        if st.code {
            f.background = theme::CODE_BG;
        }
        if st.highlight {
            f.background = Color32::from_rgba_unmultiplied(200, 170, 40, 60);
        }
        if st.strike {
            f.strikethrough = Stroke::new(1.0, theme::DONE);
            f.color = theme::DONE;
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
            let mut f = fmt(&st, theme::LINK);
            f.color = theme::LINK;
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
            let color = if *ok { theme::LINK } else { theme::LINK_MISSING };
            let mut f = fmt(&st, color);
            f.color = if st.strike { theme::DONE } else { color };
            f.underline = Stroke::new(1.0, color.gamma_multiply(0.5));
            job.append(&shown, 0.0, f);
            link_spans.push((start..job.text.len(), l.clone(), *ok));
            i = l.range.end;
            continue;
        }
        if let Some((r, _)) = tags.iter().find(|(r, _)| r.start == i) {
            let mut f = fmt(&st, theme::TAG);
            f.background = theme::TAG_BG;
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
                    job.append(&code[r.clone()], 0.0, TextFormat { color: theme::syntax_colour(kind), ..plain.clone() });
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
    let mark = TextFormat { font_id: FontId::new(size * FOOTNOTE_TEXT, crate::fonts::styled(false, false).family), color: theme::LINK, ..Default::default() };
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
    let mut job = job;
    job.wrap.max_width = ui.available_width();
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(rect.min, galley.clone(), theme::TEXT);
    paint_maths(ui, &galley, rect.min, maths);
    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
        if let Some(c) = char_under(&galley, rect.min, hp) {
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
            crate::math::paint::paint(&ui.painter().with_clip_rect(ui.clip_rect()), &f, pos, theme::TEXT);
        }
        Err(why) => {
            egui::Frame::new().fill(theme::CODE_BG).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
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
            ui.label(egui::RichText::new(&l.target).color(theme::LINK));
            ui.label(egui::RichText::new("Opens in your browser").small().color(theme::MUTED));
        });
        if resp.clicked() || resp.middle_clicked() {
            *out = Some(ReaderAction::OpenUrl(l.target.clone()));
        }
    } else if let Some((_, l, ok)) = spans.iter().find(|(r, _, _)| r.contains(&byte)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let tip = if *ok { format!("Open “{}”", l.target) } else { format!("Create note “{}”", l.target) };
        let colour = if *ok { theme::LINK } else { theme::LINK_MISSING };
        let note = hint(&l.target);
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(tip).color(colour));
            if let Some(n) = &note {
                ui.label(egui::RichText::new(n).small().color(theme::TEXT));
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
    let (job, spans, maths) = inline_job(text, &links, base, theme::TEXT, done);
    let mut out = None;
    let resp = inline_block(ui, job, &spans, &maths, &|t| vault.link_hint(t, Some(from)), &mut out);
    let plain_click = resp.clicked() && out.is_none();
    (out, plain_click)
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

/// Draw a note. `look` is the text size and whether drawings get a dark canvas.
pub fn show(ui: &mut Ui, text: &str, vault: &Vault, rel: &Path, caches: &mut Caches, look: (f32, bool), extras: &Extras) -> Option<ReaderAction> {
    let (base, dark_canvas) = look;
    let mut out = None;
    let notes = markdown::find_footnotes(text);
    let _defined = markdown::DefinedFootnotes::set(notes.defined());
    let target = jump_target(ui.ctx(), &notes);
    let hint = |t: &str| vault.link_hint(t, Some(rel));
    let all_links: Vec<(LinkSpan, bool)> = markdown::find_links(text)
        .into_iter()
        .map(|l| {
            let ok = vault.exists(&l.target, Some(rel));
            (l, ok)
        })
        .collect();
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
    while i < lines.len() {
        let span = tables.iter().find(|b| b.lines.start == i).map_or(i + 1, |b| b.lines.end);
        if target.is_some_and(|t| (i..span).contains(&t)) {
            // the end of a footnote jump (EDIT-050): this is the line
            ui.scroll_to_rect(egui::Rect::from_min_size(ui.cursor().min, Vec2::new(1.0, base * 1.5)), Some(egui::Align::TOP));
        }
        if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
            table_block(ui, b, vault, rel, base, &hint, &mut out);
            i = b.lines.end;
            continue;
        }
        // a formula on lines of its own: `$$` lines, a `math` fence, `$$…$$` alone (EDIT-046)
        if let Some(m) = formulas.iter().find(|m| m.block && m.lines.start == i) {
            math_block(ui, &text[m.tex.clone()], base);
            i = m.lines.end;
            continue;
        }
        let (off, line) = lines[i];
        let line_links: Vec<(LinkSpan, bool)> = all_links
            .iter()
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
            let lang = line.trim().trim_start_matches(['`', '~']).to_string();
            let word = syntax::fence_language(line);
            i += 1;
            while i < lines.len() && !(lines[i].1.trim_start().starts_with("```") || lines[i].1.trim_start().starts_with("~~~")) {
                code.push_str(lines[i].1);
                code.push('\n');
                i += 1;
            }
            i += 1;
            egui::Frame::new().fill(theme::CODE_BG).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !lang.is_empty() {
                    ui.label(egui::RichText::new(lang).small().color(theme::MUTED));
                }
                ui.label(code_job(&word, code.trim_end(), base * 0.9));
            });
            ui.add_space(6.0);
            continue;
        }
        let trimmed = line.trim();
        let indent = (line.len() - line.trim_start().len()) as f32 * 7.0;
        // embed on its own line
        if line_links.len() == 1 && line_links[0].0.embed && trimmed.len() == line_links[0].0.range.len() {
            let (l, ok) = &line_links[0];
            if *ok {
                if let Some(target) = vault.resolve(&l.target, Some(rel)) {
                    if crate::media::is_media(&target) {
                        // a sound is a player of its own, without the frame (MEDIA-001)
                        crate::media::widget::show(ui, caches.media, &vault.abs(&target), rel, l.embed_width());
                        ui.add_space(6.0);
                    } else {
                        embed(ui, vault, &target, l.embed_width(), caches, (base, dark_canvas), &mut out);
                    }
                    i += 1;
                    continue;
                }
            }
        }
        if trimmed.is_empty() {
            ui.add_space(base * 0.6);
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        } else if let Some(def) = notes.def_at(i) {
            // a footnote definition: its label raised, its text a little smaller (EDIT-050)
            let (job, spans, _, maths) = footnote_def_painted(line, &def.label, &def.marker, &line_links, base, theme::TEXT);
            inline_block(ui, job, &spans, &maths, &hint, &mut out);
        } else if notes.is_continuation(i) {
            let (job, spans, _, maths) = footnote_text_painted(line, &line_links, base, theme::TEXT);
            ui.horizontal_top(|ui| {
                ui.add_space(base * 1.2);
                inline_block(ui, job, &spans, &maths, &hint, &mut out);
            });
        } else if let Some(level) = markdown::heading_level(line) {
            ui.add_space(base * 0.5);
            let size = base * heading_scale(level as u8);
            let (job, spans, maths) = inline_job(line[level..].trim_start(), &shift(&line_links, level + (line[level..].len() - line[level..].trim_start().len())), size, theme::HEADING, false);
            let shown = ui.horizontal_top(|ui| {
                if let Some(n) = extras.numbers.get(&i) {
                    ui.label(egui::RichText::new(n).size(size).color(theme::MUTED));
                }
                inline_block(ui, job, &spans, &maths, &hint, &mut out)
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
                    ui.label(egui::RichText::new(format!("{}.", &t[..digits])).size(base).color(theme::ACCENT));
                }
                let mut d = done;
                if ui.checkbox(&mut d, "").clicked() {
                    out = Some(ReaderAction::ToggleTask(off + check));
                }
                let (job, spans, maths) = inline_job(rest, &shift(&line_links, rest_off), base, theme::TEXT, done);
                inline_block(ui, job, &spans, &maths, &hint, &mut out);
            });
        } else if let Some((marker, rest_off)) = list_marker(line) {
            ui.horizontal_top(|ui| {
                ui.add_space(indent + 4.0);
                ui.label(egui::RichText::new(marker).size(base).color(theme::ACCENT));
                let (job, spans, maths) = inline_job(&line[rest_off..], &shift(&line_links, rest_off), base, theme::TEXT, false);
                inline_block(ui, job, &spans, &maths, &hint, &mut out);
            });
        } else if trimmed.starts_with('>') {
            let content_off = line.find('>').unwrap() + 1;
            let content = &line[content_off..];
            let lead = content.len() - content.trim_start().len();
            egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
                let (job, spans, maths) = inline_job(content.trim_start(), &shift(&line_links, content_off + lead), base, theme::QUOTE, false);
                inline_block(ui, job, &spans, &maths, &hint, &mut out);
                let r = ui.min_rect();
                ui.painter().line_segment([r.left_top() - Vec2::new(10.0, 0.0), r.left_bottom() - Vec2::new(10.0, 0.0)], Stroke::new(3.0, theme::ACCENT.gamma_multiply(0.7)));
            });
        } else {
            let (job, spans, maths) = inline_job(line, &line_links, base, theme::TEXT, false);
            inline_block(ui, job, &spans, &maths, &hint, &mut out);
        }
        i += 1;
    }
    out
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

/// `width` is what the link asks for (`![[pic.png|300]]`, EDIT-052); `text` is the text size and
/// whether drawings get a dark canvas.
fn embed(ui: &mut Ui, vault: &Vault, target: &Path, width: Option<f32>, caches: &mut Caches, text: (f32, bool), out: &mut Option<ReaderAction>) {
    let (base, dark) = text;
    let kind = vault.file(target).map(|f| f.kind).unwrap_or(FileKind::Other);
    let name = vault.file(target).map(|f| f.display_name()).unwrap_or_default();
    let frame = egui::Frame::new().fill(theme::BG_SECONDARY).stroke(Stroke::new(1.0, theme::BORDER)).corner_radius(CornerRadius::same(8)).inner_margin(8);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&name).color(theme::MUTED).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Open").clicked() {
                    *out = Some(ReaderAction::OpenFile(target.to_path_buf()));
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
                    *out = Some(ReaderAction::OpenFile(target.to_path_buf()));
                }
            }
            FileKind::Note => {
                let text = vault.notes.get(target).map(|n| n.text.clone()).unwrap_or_default();
                let preview: String = text.lines().take(12).collect::<Vec<_>>().join("\n");
                ui.label(egui::RichText::new(preview).size(base * 0.95).color(theme::TEXT));
            }
            FileKind::Other if crate::pdf::view::is_pdf(target) => {
                let w = ui.available_width();
                match crate::pdf::view::pdf_pages(ui.ctx(), &vault.abs(target), 50) {
                    crate::pdf::view::PdfState::Ready { pages, total } => {
                        for (i, tex) in pages.iter().enumerate() {
                            let [tw, th] = tex.size_vec2().into();
                            let k = (w / tw).min(1.0);
                            let resp = ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), egui::vec2(tw * k, th * k))).sense(Sense::click()));
                            if resp.on_hover_text(format!("Page {} of {total} · click to open in your PDF viewer", i + 1)).clicked() {
                                *out = Some(ReaderAction::OpenFile(target.to_path_buf()));
                            }
                            ui.add_space(4.0);
                        }
                        if total > pages.len() {
                            ui.label(egui::RichText::new(format!("{} more pages — open the PDF to see them all", total - pages.len())).color(theme::MUTED));
                        }
                    }
                    crate::pdf::view::PdfState::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Rendering PDF…").color(theme::MUTED));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    crate::pdf::view::PdfState::Failed(e) => {
                        ui.label(egui::RichText::new(format!("This PDF can't be shown: {e}")).color(theme::MUTED));
                    }
                }
            }
            FileKind::Other if crate::media::is_image(target) => {
                let w = ui.available_width();
                // still a Git LFS pointer: "Not downloaded (12 MB) — Download" (SYNC-020)
                if crate::media::widget::show_not_downloaded(ui, caches.media, &vault.abs(target)).is_some() {
                    ui.add_space(6.0);
                    return;
                }
                let shown = match width {
                    Some(asked) => crate::media::image_widget(ui.ctx(), &vault.abs(target), asked.min(w), f32::INFINITY),
                    None => crate::media::image_widget(ui.ctx(), &vault.abs(target), w, 520.0),
                };
                match shown {
                    Some(img) => {
                        let resp = ui.add(img.corner_radius(CornerRadius::same(4)).sense(Sense::click()));
                        if resp.on_hover_text("Open with your image viewer").clicked() {
                            *out = Some(ReaderAction::OpenFile(target.to_path_buf()));
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new("This picture can't be shown (unsupported or damaged file).").color(theme::MUTED));
                    }
                }
            }
            FileKind::Other => {
                ui.label(egui::RichText::new("This kind of file can't be shown here yet.").color(theme::MUTED));
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
        assert_eq!(colours(&job, "\"foooo\""), theme::syntax_colour(syntax::Kind::String));
        assert_ne!(colours(&job, "\"foooo\""), colours(&job, "console"));
        let plain = code_job("klingon", code, 14.0);
        assert_eq!((plain.text.as_str(), colours(&plain, "\"foooo\"")), (code, theme::CODE), "an unknown language is plain monospace");
        assert_eq!(code_job("", "", 14.0).text, "", "an empty block");

        let line = "see `console.log(\"fooo\")` and `x`";
        let (job, _, _) = inline_job(line, &[], 14.0, theme::TEXT, false);
        assert_eq!(job.text, "see console.log(\"fooo\") and x", "the backticks are gone");
        assert_eq!(colours(&job, "\"fooo\""), theme::syntax_colour(syntax::Kind::String));
        assert_eq!(colours(&job, "log"), theme::syntax_colour(syntax::Kind::Function));
        assert_eq!(colours(&job, "x"), theme::CODE);
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
                    let e = Extras { numbers: extras.numbers.clone(), scroll_to: extras.scroll_to.filter(|_| n == 0), properties: extras.properties };
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
        let (job, _, paints, _) = inline_painted(line, &[], 15.0, theme::TEXT, false);
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
        let (job, _, paints, _) = inline_painted(line, &[], 15.0, theme::TEXT, false);
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
        let (job, spans, _, _) = inline_painted(line, &[], 15.0, theme::TEXT, false);
        assert_eq!(job.text, "Note1 and[^2] and [^1]");
        let mark = format_at(&job, "1 and");
        assert_eq!((mark.valign, mark.font_id.size, mark.color, mark.line_height), (egui::Align::TOP, 15.0 * SCRIPT_SCALE, theme::LINK, None));
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].0.clone(), footnote_of(&spans[0].1)), (4..5, Some((Foot::Reference, "1"))));
        assert_eq!(format_at(&job, "[^2]").valign, egui::Align::BOTTOM);
        // the same line without any definition
        drop(_defined);
        let (job, spans, _, _) = inline_painted(line, &[], 15.0, theme::TEXT, false);
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
        let (job, spans, _, _) = footnote_def_painted(line, &def.label, &def.marker, &links, 15.0, theme::TEXT);
        assert_eq!(job.text, "a See bold Ideas anda");
        let label = format_at(&job, "a See");
        assert_eq!((label.valign, label.color, label.font_id.size), (egui::Align::TOP, theme::LINK, 15.0 * FOOTNOTE_TEXT * SCRIPT_SCALE));
        assert_eq!(format_at(&job, "See").font_id.size, 15.0 * FOOTNOTE_TEXT);
        assert_eq!(format_at(&job, "bold").color, theme::TEXT_STRONG);
        let kinds: Vec<(Range<usize>, Option<Foot>, String)> = spans.iter().map(|(r, l, _)| (r.clone(), footnote_of(l).map(|f| f.0), l.target.clone())).collect();
        assert_eq!(kinds, [(0..1, Some(Foot::Definition), "a".to_string()), (11..16, None, "Ideas".to_string()), (20..21, Some(Foot::Reference), "a".to_string())]);
        // an indented line that goes on with it is smaller text
        let (job, _, _, _) = footnote_text_painted("    more *text*", &[], 15.0, theme::TEXT);
        assert_eq!(job.text, "more text");
        assert_eq!(format_at(&job, "more").font_id.size, 15.0 * FOOTNOTE_TEXT);
    }

    // spec: EDIT-045
    #[test]
    fn br_in_a_table_cell_is_a_line_break_and_elsewhere_it_is_text() {
        for tag in ["<br>", "<br/>", "<br />", "<BR>", "<Br  / >"] {
            let line = format!("• a{tag}• b");
            let (job, _, _, _) = cell_painted(&line, &[], 15.0, theme::TEXT);
            let want = if tag == "<Br  / >" { line.clone() } else { "• a\n• b".to_string() };
            assert_eq!(job.text, want, "{tag}");
        }
        let (job, _, _, _) = cell_painted("a `x<br>y` b<br>", &[], 15.0, theme::TEXT);
        assert_eq!(job.text, "a x<br>y b\n", "not in code");
        let (job, _, _, _) = inline_painted("a<br>b", &[], 15.0, theme::TEXT, false);
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
            let (job, _, _, _) = inline_painted("Claim[^1] here.", &[], 15.0, theme::TEXT, false);
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
        let (job, _, _) = crate::fonts::with_family(&font, || inline_job(line, &[], 15.0, theme::TEXT, false));
        assert_eq!((at(&job, "bold").font_id.family, at(&job, "bold").extra_letter_spacing), (fam("system:X:bold"), 0.0));
        assert_eq!((at(&job, "it").font_id.family, at(&job, "it").italics), (fam("system:X:italic"), false));
        assert_eq!(at(&job, "code").font_id.family, FontFamily::Monospace);
        // Shale's own font has no faces: bold and italic are imitated as before
        let (job, _, _) = inline_job(line, &[], 15.0, theme::TEXT, false);
        assert_eq!(at(&job, "bold").font_id.family, FontFamily::Proportional);
        assert!(at(&job, "bold").extra_letter_spacing > 0.0 && at(&job, "it").italics);
    }
}
