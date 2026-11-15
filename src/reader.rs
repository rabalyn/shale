//! Reading view: renders a note without markdown syntax. Links are clickable (blue = exists,
//! red = missing), checkboxes toggle, and `![[drawing.excalidraw]]` embeds show the drawing.

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
}

/// Turn one line of markdown into display text with formats and link positions.
pub(crate) fn inline_job(line: &str, links: &[(LinkSpan, bool)], size: f32, base_color: Color32, strike_all: bool) -> (LayoutJob, Vec<(Range<usize>, LinkSpan, bool)>) {
    let mut job = LayoutJob::default();
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
        f
    };
    // web links ride along as link spans whose target is the URL
    let web: Vec<(LinkSpan, bool)> =
        crate::lists::find_web_links(line).into_iter().map(|w| (LinkSpan { range: w.range, target_range: w.label_range, target: w.url, heading: None, alias: Some(w.label), embed: false, markdown_style: true }, true)).collect();
    let b = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
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
    if job.text.is_empty() {
        job.append(" ", 0.0, fmt(&st, base_color));
    }
    (job, link_spans)
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
            let byte = markdown::char_to_byte(&galley.job.text, c);
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
    }
    resp
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

pub fn show(ui: &mut Ui, text: &str, vault: &Vault, rel: &Path, scenes: &mut SceneCache, base: f32, dark_canvas: bool) -> Option<ReaderAction> {
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
    while i < lines.len() {
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
                    embed(ui, vault, &target, scenes, base, dark_canvas, &mut out);
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

fn embed(ui: &mut Ui, vault: &Vault, target: &Path, scenes: &mut SceneCache, base: f32, dark: bool, out: &mut Option<ReaderAction>) {
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
                if let Some(scene) = scenes.get(vault, target) {
                    let pics = crate::drawing_pictures::textures(ui.ctx(), scene, vault, target);
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
            FileKind::Other if crate::pdfview::is_pdf(target) => {
                let w = ui.available_width();
                match crate::pdfview::pdf_pages(ui.ctx(), &vault.abs(target), 50) {
                    crate::pdfview::PdfState::Ready { pages, total } => {
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
                    crate::pdfview::PdfState::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Rendering PDF…").color(theme::MUTED));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    crate::pdfview::PdfState::Failed(e) => {
                        ui.label(egui::RichText::new(format!("This PDF can't be shown: {e}")).color(theme::MUTED));
                    }
                }
            }
            FileKind::Other if crate::media::is_image(target) => {
                let w = ui.available_width();
                match crate::media::image_widget(ui.ctx(), &vault.abs(target), w, 520.0) {
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
