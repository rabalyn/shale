//! The comparison view (HIST-008…HIST-015): the note with its changes marked, or the Markdown side
//! by side, over the note, with the actions and the slider.

use super::diff::{self, Change, Kind, Mark, Row, Shown, Side, Span};
use super::labels::{day_time, source_label};
use super::state::{Changes, Data, Mode};
use crate::app::ShaleApp;
use crate::clickable::ComboBoxPointer;
use crate::gitsync::history::Version;
use crate::theme;
use eframe::egui::{self, text::LayoutJob, Align, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextFormat, Ui, Vec2};

/// What a click in the view asked for.
enum Do {
    From(String),
    To(Option<String>),
    Mode(Mode),
    Step(isize),
    Restore,
    Copy,
    Alone,
    Back,
    CopyPatch(String),
    CopyAlone(String),
    OpenFold(usize),
}

impl ShaleApp {
    /// Draw the view and do what was clicked; false: close it.
    pub(crate) fn history_changes_dialog(&mut self, ctx: &egui::Context) -> bool {
        let mut ch = std::mem::take(&mut self.history.changes);
        let versions = std::mem::take(&mut self.history.panel.versions);
        ch.rects.clear();
        let now = crate::calendar::time::now();
        let this_device = crate::gitsync::host_name();
        let dark = self.cfg.dark_canvas;
        let title = title_of(&ch, &versions, now);
        let mut asked: Vec<Do> = vec![];
        let resp = crate::dialog::Dialog::new("history-changes", title).width(980.0).show(ctx, |ui| {
            asked = body(ui, &mut ch, &versions, now, &this_device, dark);
        });
        self.history.changes = ch;
        self.history.panel.versions = versions;
        let mut keep = !resp.should_close();
        for a in asked {
            match a {
                Do::From(commit) => {
                    self.history.changes.from = commit;
                    self.history.changes.alone = None;
                    // the newer end must stay newer than the older one
                    if let Some(to) = self.history.changes.to.clone() {
                        let pos = |c: &str| self.history.panel.versions.iter().position(|v| v.commit == c);
                        if pos(&to).zip(pos(&self.history.changes.from)).is_none_or(|(t, f)| t >= f) {
                            self.history.changes.to = None;
                        }
                    }
                    self.history_compare();
                }
                Do::To(to) => {
                    self.history.changes.to = to;
                    self.history_compare();
                }
                Do::Mode(m) => {
                    self.history.changes.mode = m;
                    self.history.changes.current = 0;
                    self.history.changes.jump = true;
                }
                Do::Step(d) => {
                    let ch = &mut self.history.changes;
                    let n = match (&ch.data, ch.mode) {
                        (Some(Data::Text(t)), Mode::Source) => diff::side_change_starts(&t.side).len(),
                        _ => ch.starts().len(),
                    };
                    ch.current = ch.current.saturating_add_signed(d).min(n.saturating_sub(1));
                    ch.jump = true;
                }
                Do::Restore | Do::Copy | Do::Alone => {
                    let from = self.history.panel.versions.iter().find(|v| v.commit == self.history.changes.from).cloned();
                    let rel = self.history.changes.rel.clone();
                    if let Some(v) = from {
                        match a {
                            Do::Restore => {
                                self.history_restore_version(&rel, &v);
                                keep = false;
                            }
                            Do::Copy => self.history_copy_version(&v),
                            _ => self.history_view_version(&v),
                        }
                    }
                }
                Do::Back => self.history.changes.alone = None,
                Do::CopyPatch(text) | Do::CopyAlone(text) => ctx.copy_text(text),
                Do::OpenFold(at) => {
                    self.history.changes.open_folds.insert(at);
                }
            }
        }
        keep
    }
}

/// "Changes since 11:05", or "Changes from 11:05 to 14:32" (HIST-008).
fn title_of(ch: &Changes, versions: &[Version], now: i64) -> String {
    let at = |c: &str| versions.iter().find(|v| v.commit == c).map(|v| day_time(v.time, now));
    let name = crate::vault::rel_to_string(&ch.rel);
    match (at(&ch.from), ch.to.as_deref().and_then(at)) {
        (Some(a), Some(b)) => format!("Changes from {a} to {b} \u{2013} {name}"),
        (Some(a), None) => format!("Changes since {a} \u{2013} {name}"),
        _ => format!("Changes \u{2013} {name}"),
    }
}

fn body(ui: &mut Ui, ch: &mut Changes, versions: &[Version], now: i64, this_device: &str, dark: bool) -> Vec<Do> {
    let mut asked = vec![];
    if let Some((title, text)) = &ch.alone {
        ui.label(egui::RichText::new(title).strong());
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            let mut shown: &str = text;
            ui.add(egui::TextEdit::multiline(&mut shown).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
        ui.horizontal(|ui| {
            let back = ui.button("Back to the changes");
            ch.rects.push(("back".into(), back.rect));
            if back.clicked() {
                asked.push(Do::Back);
            }
            let copy = ui.button("Copy text");
            ch.rects.push(("copy-shown".into(), copy.rect));
            if copy.clicked() {
                asked.push(Do::CopyAlone(text.clone()));
            }
        });
        return asked;
    }
    let from_pos = versions.iter().position(|v| v.commit == ch.from);
    let to_pos = ch.to.as_ref().and_then(|c| versions.iter().position(|v| &v.commit == c));
    let text_data = matches!(ch.data, Some(Data::Text(_)));
    // the switch, and the actions at the right
    ui.horizontal(|ui| {
        for (key, label, mode) in [("mode-changes", "Changes", Mode::Changes), ("mode-source", "Source, side by side", Mode::Source)] {
            let b = ui.add_enabled(text_data || ch.mode == mode, egui::Button::selectable(ch.mode == mode, label));
            ch.rects.push((key.into(), b.rect));
            if b.clicked() && ch.mode != mode {
                asked.push(Do::Mode(mode));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let ready = !ch.loading && from_pos.is_some();
            let b = ui.add_enabled(ready, egui::Button::new("Restore this version").fill(theme::palette().accent.gamma_multiply(0.35)));
            ch.rects.push(("restore".into(), b.rect));
            if b.clicked() {
                asked.push(Do::Restore);
            }
            let b = ui.add_enabled(ready, egui::Button::new("Read-only"));
            ch.rects.push(("read-only".into(), b.rect));
            if b.clicked() {
                asked.push(Do::Alone);
            }
            let b = ui.add_enabled(ready, egui::Button::new("Copy text"));
            ch.rects.push(("copy".into(), b.rect));
            if b.clicked() {
                asked.push(Do::Copy);
            }
            if ch.mode == Mode::Source {
                if let Some(Data::Text(t)) = &ch.data {
                    let b = ui.add_enabled(!t.patch.is_empty(), egui::Button::new("Copy as patch"));
                    ch.rects.push(("patch".into(), b.rect));
                    if b.clicked() {
                        asked.push(Do::CopyPatch(t.patch.clone()));
                    }
                }
            }
        });
    });
    // the time slider over all versions, oldest at the left, and where the newer end is
    ui.horizontal(|ui| {
        let n = versions.len();
        if let (Some(fp), true) = (from_pos, n >= 2) {
            let current = n - 1 - fp;
            let mut c = ch.scrub.unwrap_or(current);
            ui.label("Since");
            let slider = ui.add(egui::Slider::new(&mut c, 0..=n - 1).show_value(false));
            ch.rects.push(("slider".into(), slider.rect));
            if slider.dragged() {
                ch.scrub = Some(c);
            } else if slider.changed() || slider.drag_stopped() {
                ch.scrub = None;
                if c != current {
                    asked.push(Do::From(versions[n - 1 - c].commit.clone()));
                }
            }
            let shown = &versions[n - 1 - c.min(n - 1)];
            ui.label(egui::RichText::new(format!("{} \u{b7} {}", day_time(shown.time, now), source_label(versions, n - 1 - c.min(n - 1), this_device, now))).color(theme::palette().muted));
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let to_text = to_pos.map_or("Now".to_string(), |p| day_time(versions[p].time, now));
            let combo = egui::ComboBox::from_id_salt("history-to").selected_text(to_text).show_pointer(ui, |ui| {
                if ui.selectable_label(to_pos.is_none(), "Now").clicked() {
                    asked.push(Do::To(None));
                }
                for (i, v) in versions.iter().enumerate().take(from_pos.unwrap_or(0)) {
                    if ui.selectable_label(to_pos == Some(i), format!("{} \u{b7} {}", day_time(v.time, now), source_label(versions, i, this_device, now))).clicked() {
                        asked.push(Do::To(Some(v.commit.clone())));
                    }
                }
            });
            ch.rects.push(("to".into(), combo.response.rect));
            ui.label("to");
        });
    });
    // next and previous change
    ui.horizontal(|ui| {
        let starts = match (&ch.data, ch.mode) {
            (Some(Data::Text(t)), Mode::Source) => diff::side_change_starts(&t.side),
            _ => ch.starts(),
        };
        let n = starts.len();
        let b = ui.add_enabled(n > 0 && ch.current > 0, egui::Button::new("\u{25c0} Previous change"));
        ch.rects.push(("prev".into(), b.rect));
        if b.clicked() {
            asked.push(Do::Step(-1));
        }
        let b = ui.add_enabled(n > 0 && ch.current + 1 < n, egui::Button::new("Next change \u{25b6}"));
        ch.rects.push(("next".into(), b.rect));
        if b.clicked() {
            asked.push(Do::Step(1));
        }
        if n > 0 {
            ui.label(egui::RichText::new(format!("{} of {n}", ch.current + 1)).color(theme::palette().muted));
        }
        if ch.mode == Mode::Source {
            // only here the two versions are named by their short hashes (HIST-012)
            let short = |c: &str| c.chars().take(7).collect::<String>();
            let from = short(&ch.from);
            let to = ch.to.as_deref().map_or("the note now".to_string(), short);
            ui.label(egui::RichText::new(format!("{from} \u{2192} {to}")).monospace().size(11.0).color(theme::palette().muted)).on_hover_text(format!("{} \u{2192} {}", ch.from, ch.to.as_deref().unwrap_or("the note as it is now")));
        }
    });
    ui.separator();
    if ch.loading {
        theme::hint(ui, "Reading the versions\u{2026}");
        return asked;
    }
    if let Some(e) = &ch.error {
        theme::problem(ui, e.clone());
        return asked;
    }
    let jump = std::mem::take(&mut ch.jump);
    match &ch.data {
        None => {}
        Some(Data::Text(t)) if ch.mode == Mode::Changes => {
            let starts = diff::change_starts(&t.rows);
            let shown = diff::fold(&t.rows, &ch.open_folds);
            let (cur_from, cur_to) = current_run(&t.rows, &starts, ch.current);
            if starts.is_empty() {
                theme::hint(ui, "No changes: the note is the same as in that version.");
            }
            let mut fold_open = None;
            egui::ScrollArea::vertical().id_salt("history-changes-body").max_height(ui.available_height().max(240.0)).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for s in &shown {
                    match s {
                        Shown::Row(i) => {
                            let rect = draw_row(ui, &t.rows[*i]);
                            if (cur_from..cur_to).contains(i) {
                                ui.painter().rect_stroke(rect.expand(3.0), 4.0, Stroke::new(1.5, theme::palette().accent), egui::StrokeKind::Outside);
                            }
                            if jump && *i == cur_from && !starts.is_empty() {
                                ui.scroll_to_rect(rect, Some(Align::Center));
                            }
                        }
                        Shown::Fold { from, to } => {
                            let text = Shown::fold_text(&t.rows, *from, *to);
                            let b = ui.add(egui::Button::new(egui::RichText::new(text).color(theme::palette().muted)).fill(theme::palette().faint_bg).min_size(Vec2::new(ui.available_width(), 24.0)));
                            if b.clicked() {
                                fold_open = Some(*from);
                            }
                        }
                    }
                }
            });
            if let Some(at) = fold_open {
                asked.push(Do::OpenFold(at));
            }
        }
        Some(Data::Text(t)) => {
            let starts = diff::side_change_starts(&t.side);
            source_view(ui, &t.side, starts.get(ch.current).copied().filter(|_| jump));
        }
        Some(Data::Drawing { before, after }) => {
            let now_label = ch.to.as_ref().and_then(|c| versions.iter().find(|v| &v.commit == c)).map_or("Now".to_string(), |v| day_time(v.time, now));
            let before_label = versions.iter().find(|v| v.commit == ch.from).map_or("Before".to_string(), |v| day_time(v.time, now));
            let (b, a) = (before.live().count(), after.live().count());
            ui.label(if b == a && before.bounds() == after.bounds() { "The drawing has not changed." } else { "The drawing changed." });
            ui.horizontal_top(|ui| {
                let w = (ui.available_width() - 12.0) / 2.0;
                for (label, scene) in [(before_label, before), (now_label, after)] {
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.label(egui::RichText::new(format!("{label} \u{b7} {} elements", scene.live().count())).color(theme::palette().muted));
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 320.0), Sense::hover());
                        ui.painter().rect_filled(rect, 6.0, if dark { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE });
                        crate::drawing::paint_thumbnail(ui.painter(), scene, rect.shrink(6.0), dark, &crate::drawing::pictures::Pictures::new());
                    });
                }
            });
        }
        Some(Data::Binary { before, after }) => {
            ui.label("This version changed. It is not text, so its changes are not shown.");
            ui.horizontal_top(|ui| {
                let w = (ui.available_width() - 12.0) / 2.0;
                for (label, size) in [("Before", before), ("After", after)] {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 120.0), Sense::hover());
                    ui.painter().rect_filled(rect, 6.0, theme::palette().faint_bg);
                    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, theme::palette().border), egui::StrokeKind::Inside);
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, format!("{label}\n{}", crate::data_usage::human_bytes(*size as u64)), FontId::proportional(14.0), theme::palette().muted);
                }
            });
        }
    }
    asked
}

/// The rows of the change the view is at: from its first row to the end of its run.
fn current_run(rows: &[Row], starts: &[usize], current: usize) -> (usize, usize) {
    match starts.get(current) {
        Some(&from) => (from, (from..rows.len()).find(|&i| rows[i].mark == Mark::Same).unwrap_or(rows.len())),
        None => (0, 0),
    }
}

fn gutter_colours(mark: Mark) -> (Color32, Color32, &'static str) {
    let p = theme::palette();
    match mark {
        Mark::Added => (p.good_bg, p.good, "+"),
        Mark::Removed => (p.bad_bg, p.link_missing, "\u{2212}"),
        _ => (p.warn_bg, p.warn, "~"),
    }
}

fn span_format(span: &Span, font: &FontId, colour: Color32) -> TextFormat {
    let p = theme::palette();
    let mut f = TextFormat { font_id: font.clone(), color: colour, ..Default::default() };
    match span.change {
        Change::Same => {}
        Change::Ins => {
            f.background = p.good_bg;
            f.underline = Stroke::new(1.0, p.good);
        }
        Change::Del => {
            f.background = p.bad_bg;
            f.strikethrough = Stroke::new(1.0, p.link_missing);
        }
    }
    f
}

/// One block of the track-changes view: a gutter mark and the text (HIST-009, HIST-011).
pub(crate) fn draw_row(ui: &mut Ui, row: &Row) -> Rect {
    let p = theme::palette();
    let top = ui.cursor().min;
    let (font, colour) = match &row.kind {
        Kind::Heading(1) => (FontId::proportional(24.0), p.text_strong),
        Kind::Heading(2) => (FontId::proportional(20.0), p.text_strong),
        Kind::Heading(_) => (FontId::proportional(17.0), p.text_strong),
        Kind::Code => (FontId::monospace(13.0), p.code),
        Kind::Property => (FontId::monospace(12.5), p.muted),
        Kind::Quote => (FontId::proportional(14.5), p.quote),
        _ => (FontId::proportional(14.5), p.text),
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (gutter, resp) = ui.allocate_exact_size(Vec2::new(22.0, 20.0), Sense::hover());
        if row.mark != Mark::Same {
            let (fill, edge, glyph) = gutter_colours(row.mark);
            ui.painter().rect_filled(gutter, 4.0, fill);
            ui.painter().rect_stroke(gutter, 4.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
            ui.painter().text(gutter.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(14.0), p.text_strong);
            let what = match row.mark {
                Mark::Added => "Added",
                Mark::Removed => "Removed",
                _ => "Changed",
            };
            resp.on_hover_text(format!("{what} {}", row.kind.name()));
        }
        if let Kind::Item(depth, _) = &row.kind {
            ui.add_space(*depth as f32 * 20.0);
        }
        let mut job = LayoutJob::default();
        let plain = TextFormat { font_id: font.clone(), color: colour, ..Default::default() };
        match &row.kind {
            Kind::Item(_, bullet) => job.append(&format!("{bullet}  "), 0.0, TextFormat { color: p.muted, ..plain.clone() }),
            Kind::Quote => job.append("\u{258e} ", 0.0, TextFormat { color: p.muted, ..plain.clone() }),
            _ => {}
        }
        for s in &row.spans {
            job.append(&s.text, 0.0, span_format(s, &font, colour));
        }
        if row.kind == Kind::Rule {
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, p.border));
        } else {
            ui.add(egui::Label::new(job).wrap());
        }
    });
    Rect::from_min_max(top, Pos2::new(ui.min_rect().max.x, ui.cursor().min.y))
}

const LINE_H: f32 = 18.0;
const NUM_W: f32 = 40.0;
const MARK_W: f32 = 18.0;

/// The Markdown side by side, with line numbers, tinted changed lines, a mark between the sides and
/// the changed words underlined or struck through (HIST-012). Only the rows in view are drawn.
pub(crate) fn source_view(ui: &mut Ui, rows: &[Side], jump_to: Option<usize>) {
    let mut area = egui::ScrollArea::both().id_salt("history-source-body").auto_shrink([false, false]);
    if let Some(row) = jump_to {
        area = area.vertical_scroll_offset((row as f32 * LINE_H - 60.0).max(0.0));
    }
    let half = ((ui.available_width() - 2.0 * NUM_W - MARK_W) / 2.0).max(200.0);
    area.show_rows(ui, LINE_H, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in &rows[range] {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(2.0 * (NUM_W + half) + MARK_W, LINE_H), Sense::hover());
            let left = Rect::from_min_size(rect.min, Vec2::new(NUM_W + half, LINE_H));
            let right = Rect::from_min_size(Pos2::new(rect.min.x + NUM_W + half + MARK_W, rect.min.y), Vec2::new(NUM_W + half, LINE_H));
            side_half(ui, left, row.left.as_ref(), matches!(row.mark, Mark::Removed | Mark::Changed), theme::palette().bad_bg);
            side_half(ui, right, row.right.as_ref(), matches!(row.mark, Mark::Added | Mark::Changed), theme::palette().good_bg);
            if row.mark != Mark::Same {
                let (_, edge, glyph) = gutter_colours(row.mark);
                let mid = Rect::from_min_size(Pos2::new(left.max.x, rect.min.y), Vec2::new(MARK_W, LINE_H));
                ui.painter().text(mid.center(), Align2::CENTER_CENTER, glyph, FontId::monospace(13.0), edge);
            }
        }
    });
}

fn side_half(ui: &mut Ui, rect: Rect, line: Option<&(usize, Vec<Span>)>, tint: bool, fill: Color32) {
    let p = theme::palette();
    if tint {
        ui.painter().rect_filled(rect, 0.0, fill.gamma_multiply(0.5));
    }
    let Some((number, spans)) = line else {
        ui.painter().rect_filled(rect, 0.0, p.faint_bg);
        return;
    };
    ui.painter().text(Pos2::new(rect.min.x + NUM_W - 6.0, rect.center().y), Align2::RIGHT_CENTER, number.to_string(), FontId::monospace(11.5), p.faint);
    let font = FontId::monospace(12.5);
    let mut job = LayoutJob::default();
    for s in spans {
        job.append(&s.text, 0.0, span_format(s, &font, p.text));
    }
    job.wrap.max_width = f32::INFINITY;
    let galley = ui.painter().layout_job(job);
    let text_area = Rect::from_min_max(Pos2::new(rect.min.x + NUM_W, rect.min.y), rect.max);
    ui.painter().with_clip_rect(text_area).galley(Pos2::new(text_area.min.x, rect.min.y + (LINE_H - galley.size().y) / 2.0), galley, p.text);
}

#[cfg(test)]
mod tests {
    use crate::themes::Themes;
    use eframe::egui::Color32;

    /// The contrast ratio of WCAG 2, 1 to 21.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let lin = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (la, lb) = (lum(a), lum(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    // spec: HIST-009, HIST-011
    #[test]
    fn hist_009_the_colours_of_the_changes_are_readable_in_every_dark_and_light_theme() {
        let themes = Themes::load(None);
        let (mut dark, mut light) = (0, 0);
        for t in &themes.list {
            let p = &t.palette;
            if p.dark {
                dark += 1;
            } else {
                light += 1;
            }
            // text on a tinted background keeps four fifths of the contrast of the theme's text on its own background
            // (some themes, like Solarized, are soft by design), and never under 3:1
            let own = contrast(p.text, p.bg_primary);
            let floor = (own * 0.8).clamp(3.0, 4.5);
            for (name, bg) in [("inserted", p.good_bg), ("deleted", p.bad_bg), ("changed", p.warn_bg)] {
                assert!(contrast(p.text, bg) >= floor, "{}: text on the {name} background: {:.1} (own {own:.1})", t.name, contrast(p.text, bg));
                assert!(contrast(p.text_strong, bg) >= floor, "{}: the gutter mark on {name}: {:.1}", t.name, contrast(p.text_strong, bg));
            }
            // the underline and the strike-through are lines, not text: 3:1 is enough
            for (name, line) in [("underline", p.good), ("strike-through", p.link_missing)] {
                assert!(contrast(line, p.bg_primary) >= 3.0, "{}: the {name}: {:.1}", t.name, contrast(line, p.bg_primary));
            }
        }
        assert!(dark >= 5 && light >= 3, "dark and light themes are both checked: {dark} {light}");
    }
}
