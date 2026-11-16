//! Reading view: renders a note without markdown syntax. Links are clickable (blue = exists,
//! red = missing), checkboxes toggle, and `![[drawing.excalidraw]]` embeds show the drawing.

use crate::colour_tags;
use crate::drawing::{self, Scene};
use crate::editor::{char_under, heading_scale};
use crate::markdown::{self, LinkSpan};
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontFamily, FontId, Sense, Stroke, TextFormat, Ui, Vec2};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub enum ReaderAction {
    OpenLink {
        target: String,
        new_tab: bool,
    },
    ToggleTask(usize),
    OpenFile(PathBuf),
    /// A web link: open in the default browser.
    OpenUrl(String),
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
}

/// Where a line's text got its own colours (EDIT-029): its bytes in the job's text, the background
/// and the text colour as written. The PDF export draws these exactly as written.
pub(crate) type Paint = (Range<usize>, Option<[u8; 4]>, Option<[u8; 4]>);

/// A line as a layout job, the links in it and its coloured text.
pub(crate) type Painted = (LayoutJob, Vec<(Range<usize>, LinkSpan, bool)>, Vec<Paint>);

/// Turn one line of markdown into display text with formats and link positions.
pub(crate) fn inline_job(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool) -> (LayoutJob, Vec<(Range<usize>, LinkSpan, bool)>) {
    let (job, spans, _) = inline_painted(line, links, size, base_color, strike_all);
    (job, spans)
}

/// `inline_job`, and the text that has colours of its own.
pub(crate) fn inline_painted(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool) -> Painted {
    let mut job = LayoutJob::default();
    let mut paints: Vec<Paint> = Vec::new();
    let runs = colour_tags::find(line);
    let mut link_spans = Vec::new();
    let tags = markdown::find_tags(line);
    let mut st = Inline { strike: strike_all, ..Default::default() };
    let fmt = |st: &Inline, color: Color32| {
        let mut f = TextFormat {
            font_id: if st.code { FontId::new(size * 0.92, FontFamily::Monospace) } else { FontId::new(size, FontFamily::Proportional) },
            color: if st.code {
                theme::CODE
            } else if st.bold {
                theme::TEXT_STRONG
            } else {
                color
            },
            italics: st.italic,
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
        if st.bold {
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
        prev = (job.text.len(), st.bg, st.fg);
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
                job.append(&rest[1..1 + e], 0.0, fmt(&s2, base_color));
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
    (job, link_spans, paints)
}

/// Paint an inline job as a wrapped block and handle link clicks.
/// `hint` gives an extra tooltip line for a link target (LINK-008, LINK-012).
fn inline_block(ui: &mut Ui, job: LayoutJob, spans: &[(Range<usize>, LinkSpan, bool)], hint: &dyn Fn(&str) -> Option<String>, out: &mut Option<ReaderAction>) -> egui::Response {
    let mut job = job;
    job.wrap.max_width = ui.available_width();
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(rect.min, galley.clone(), theme::TEXT);
    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) {
        if let Some(c) = char_under(&galley, rect.min, hp) {
            follow_span(ui, &resp, markdown::char_to_byte(&galley.job.text, c), spans, hint, out);
        }
    }
    resp
}

/// Hover and click on the link at byte `byte` of a block's text (if there is one): the pointer,
/// the tooltip and the action (web links open the browser, notes open or are created).
fn follow_span(ui: &Ui, resp: &egui::Response, byte: usize, spans: &[(Range<usize>, LinkSpan, bool)], hint: &dyn Fn(&str) -> Option<String>, out: &mut Option<ReaderAction>) {
    if let Some((_, l, _)) = spans.iter().find(|(r, l, _)| r.contains(&byte) && crate::lists::is_web_url(&l.target)) {
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
            *out = Some(ReaderAction::OpenLink { target: l.target.clone(), new_tab });
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
    let (job, spans) = inline_job(text, &links, base, theme::TEXT, done);
    let mut out = None;
    let resp = inline_block(ui, job, &spans, &|t| vault.link_hint(t, Some(from)), &mut out);
    let plain_click = resp.clicked() && out.is_none();
    (out, plain_click)
}

pub fn show(ui: &mut Ui, text: &str, vault: &Vault, rel: &Path, caches: &mut Caches, base: f32, dark_canvas: bool) -> Option<ReaderAction> {
    let mut out = None;
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
            egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(CornerRadius::same(6)).inner_margin(8).show(ui, |ui| {
                ui.label(egui::RichText::new("Properties").small().color(theme::MUTED));
                for (_, l) in &lines[1..end + 1] {
                    ui.label(egui::RichText::new(*l).monospace().color(theme::TEXT));
                }
            });
            ui.add_space(8.0);
            i = end + 2;
        }
    }
    let tables = crate::table::find_tables(text);
    while i < lines.len() {
        if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
            table_block(ui, b, vault, rel, base, &hint, &mut out);
            i = b.lines.end;
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
                ui.label(egui::RichText::new(code.trim_end()).monospace().size(base * 0.9).color(theme::CODE));
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
        } else if let Some(level) = markdown::heading_level(line) {
            ui.add_space(base * 0.5);
            let size = base * heading_scale(level as u8);
            let (job, spans) = inline_job(line[level..].trim_start(), &shift(&line_links, level + (line[level..].len() - line[level..].trim_start().len())), size, theme::HEADING, false);
            inline_block(ui, job, &spans, &hint, &mut out);
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
                let (job, spans) = inline_job(rest, &shift(&line_links, rest_off), base, theme::TEXT, done);
                inline_block(ui, job, &spans, &hint, &mut out);
            });
        } else if let Some((marker, rest_off)) = list_marker(line) {
            ui.horizontal_top(|ui| {
                ui.add_space(indent + 4.0);
                ui.label(egui::RichText::new(marker).size(base).color(theme::ACCENT));
                let (job, spans) = inline_job(&line[rest_off..], &shift(&line_links, rest_off), base, theme::TEXT, false);
                inline_block(ui, job, &spans, &hint, &mut out);
            });
        } else if trimmed.starts_with('>') {
            let content_off = line.find('>').unwrap() + 1;
            let content = &line[content_off..];
            let lead = content.len() - content.trim_start().len();
            egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
                let (job, spans) = inline_job(content.trim_start(), &shift(&line_links, content_off + lead), base, theme::QUOTE, false);
                inline_block(ui, job, &spans, &hint, &mut out);
                let r = ui.min_rect();
                ui.painter().line_segment([r.left_top() - Vec2::new(10.0, 0.0), r.left_bottom() - Vec2::new(10.0, 0.0)], Stroke::new(3.0, theme::ACCENT.gamma_multiply(0.7)));
            });
        } else {
            let (job, spans) = inline_job(line, &line_links, base, theme::TEXT, false);
            inline_block(ui, job, &spans, &hint, &mut out);
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
                follow_span(ui, &resp, markdown::char_to_byte(&cell.galley.job.text, ch), &cell.spans, hint, out);
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

/// `width` is what the link asks for (`![[pic.png|300]]`, EDIT-024); `text` is the text size and
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
                action = show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, 15.0, true);
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

    // spec: EDIT-029
    #[test]
    fn the_reading_view_colours_the_text_and_hides_the_tags() {
        let line = "a <mark style=\"background: #FFFF00;\">hi <span style=\"color: #FF0000;\">there</span></mark> **b** <mark style=\"background: #FFFF00\">open";
        let (job, _, paints) = inline_painted(line, &[], 15.0, theme::TEXT, false);
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
}
