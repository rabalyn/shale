//! A typeset formula on the screen (EDIT-048): its mask at the pixel size of the screen as a
//! texture, tinted with the text colour. A texture is made once per formula and screen scale.

use super::raster::{rasterise, PAD};
use super::Formula;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, TextureOptions, Vec2};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The most pictures kept; past it the ones that were not drawn lately are let go.
const KEEP: usize = 256;

/// The picture of a formula at one screen scale.
struct Picture {
    tex: egui::TextureHandle,
    /// in pixels
    size: [usize; 2],
    /// the pixels per point it was made at
    scale: f32,
    /// pixels from the top of the picture to the baseline
    baseline: f32,
    /// the pass it was drawn in last
    used: u64,
}

#[derive(Clone, Default)]
struct Textures(Arc<Mutex<HashMap<(u64, u32), Picture>>>);

/// The size of the formula's box in points: its width and its height (ascent and descent).
pub fn size(f: &Formula) -> Vec2 {
    Vec2::new(f.layout.width, f.layout.height())
}

/// How many pictures are kept for this screen (tests).
#[cfg(test)]
pub fn pictures_kept(ctx: &egui::Context) -> usize {
    let store: Textures = ctx.data_mut(|d| d.get_temp_mut_or_default::<Textures>(egui::Id::unique("shale-math-pictures")).clone());
    let n = store.0.lock().map_or(0, |m| m.len());
    n
}

/// Draw `f` with the left end of its baseline at `origin` (points), in `color`. The picture is made
/// the first time (EDIT-048) and drawn from then on.
pub fn paint(painter: &Painter, f: &Formula, origin: Pos2, color: Color32) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let store: Textures = ctx.data_mut(|d| d.get_temp_mut_or_default::<Textures>(egui::Id::unique("shale-math-pictures")).clone());
    let now = ctx.cumulative_pass_nr();
    let mut map = store.0.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= KEEP {
        map.retain(|_, p| p.used + 4 >= now);
    }
    let pic = map.entry((f.id, ppp.to_bits())).or_insert_with(|| {
        let mask = rasterise(&f.layout, ppp);
        let rgba: Vec<u8> = mask.alpha.iter().flat_map(|a| [*a, *a, *a, *a]).collect();
        let image = egui::ColorImage::from_rgba_premultiplied([mask.width, mask.height], &rgba);
        Picture { tex: ctx.load_texture("math", image, TextureOptions::LINEAR), size: [mask.width, mask.height], scale: mask.scale, baseline: mask.baseline, used: now }
    });
    pic.used = now;
    // on the pixel grid, so the strokes stay as sharp as they were made
    let left = (origin.x * ppp).round() / ppp - PAD as f32 / pic.scale;
    let top = (origin.y * ppp).round() / ppp - pic.baseline / pic.scale;
    let rect = Rect::from_min_size(Pos2::new(left, top), Vec2::new(pic.size[0] as f32, pic.size[1] as f32) / pic.scale);
    painter.image(pic.tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), color);
}

/// The font size of the hidden TeX that a formula takes the place of in a line of text: so small
/// that the text of a formula adds almost no width of its own.
pub const HIDDEN_SIZE: f32 = 0.1;

/// What a row of text with an em of `em` points needs to hold `f` inline: how much taller the line
/// boxes of its text must be, and the line height of the hidden section of the formula.
///
/// The text of a row sits at the bottom of it, its baseline 0.33 to 0.45 em above the bottom (by
/// the font) when its line box is 1.5 em tall; a taller box lowers the bottom by half of the
/// difference. So the formula's depth below the baseline needs a taller box for the text, and its
/// height above it a taller row.
pub fn room_for(f: &Formula, em: f32) -> (f32, f32) {
    let extra = 2.0 * (f.layout.descent + 1.0 - 0.33 * em).max(0.0);
    let hidden = (1.5 * em + extra).max(f.layout.ascent + 1.0 + 0.45 * em + extra / 2.0);
    (extra, hidden)
}

/// The text format of the hidden TeX of a formula in a line (see [`room_for`]): invisible.
pub fn hidden_format(f: &Formula, em: f32) -> egui::text::TextFormat {
    egui::text::TextFormat { font_id: egui::FontId::proportional(HIDDEN_SIZE), color: Color32::TRANSPARENT, line_height: Some(room_for(f, em).1), ..Default::default() }
}

/// Whether a text format is that of hidden TeX (see [`hidden_format`]) or of the hidden link text
/// of a picture inside a line (EDIT-114): invisible, tiny, with room before it.
pub fn is_hidden_formula(format: &egui::text::TextFormat) -> bool {
    format.color == Color32::TRANSPARENT && format.font_id.size == HIDDEN_SIZE
}

/// Where the second part of the hidden TeX of a formula starts: its first character (`$`) is a
/// section of its own. The room for the formula is the leading space of the second part: that of a
/// section that begins a row is dropped by the text layout, a character before it keeps it.
pub fn room_starts(tex_with_dollars: &str) -> usize {
    tex_with_dollars.chars().next().map_or(0, char::len_utf8)
}

/// Add the hidden TeX of a formula (`text`, with its `$`s) to `job`: invisible, with room for the
/// formula before what follows (see [`room_starts`]).
pub fn append_hidden(job: &mut egui::text::LayoutJob, text: &str, f: &Formula, em: f32) {
    let format = hidden_format(f, em);
    let first = room_starts(text);
    job.append(&text[..first], 0.0, format.clone());
    job.append(&text[first..], f.layout.width, format);
}

/// The text format of the hidden TeX of a display formula that takes whole lines: its row is as
/// tall as the formula and room around it (the formula is drawn centred in it).
pub fn block_format(f: &Formula, em: f32) -> egui::text::TextFormat {
    egui::text::TextFormat { font_id: egui::FontId::proportional(HIDDEN_SIZE), color: Color32::TRANSPARENT, line_height: Some(f.layout.height() + em), ..Default::default() }
}

/// Draw `f` in the room that its hidden text (see [`hidden_format`]) has in `galley`, on the baseline of the text of its row. `at` is the char index of the hidden
/// text, `pos` the position of the galley, `em` the size of the text around.
pub fn paint_in_galley(painter: &Painter, galley: &egui::Galley, pos: Pos2, at: usize, f: &Formula, color: Color32) {
    let Some((x, y)) = place_in_galley(galley, at, f) else { return };
    paint(painter, f, pos + Vec2::new(x, y), color);
}

/// Where the left end of the baseline of `f` is in `galley` (see [`paint_in_galley`]).
pub fn place_in_galley(galley: &egui::Galley, at: usize, f: &Formula) -> Option<(f32, f32)> {
    place_in_row(galley, at, |height| (height + f.layout.ascent - f.layout.descent) / 2.0)
}

/// Where the char `at` of `galley` is: its x and the baseline of the text of its row. A row of
/// nothing but hidden text has no text to set the baseline: `alone` gives it from the row's height.
pub fn place_in_row(galley: &egui::Galley, at: usize, alone: impl Fn(f32) -> f32) -> Option<(f32, f32)> {
    let c = galley.pos_from_cursor(egui::text::CCursor::new(at));
    let mut first = 0;
    let row = galley.rows.iter().find(|row| {
        first += row.char_count_including_newline().0;
        at < first
    })?;
    let main = row.row.glyphs.iter().filter(|g| g.font_height > 2.0).reduce(|a, b| if b.font_height > a.font_height { b } else { a });
    let baseline = main.map_or_else(|| alone(row.row.size.y), |g| g.pos.y);
    Some((c.min.x, row.pos.y + baseline))
}

/// Draw the display formula `f` centred between `left` and `right` (x, in the space of the
/// screen) in the row of `galley` that holds the char `at`, which `block_format` made as tall as it.
pub fn paint_block_in_galley(painter: &Painter, galley: &egui::Galley, pos: Pos2, at: usize, span: (f32, f32), f: &Formula, color: Color32) {
    let mut first = 0;
    let Some(row) = galley.rows.iter().find(|row| {
        first += row.char_count_including_newline().0;
        at < first
    }) else {
        return;
    };
    let y = pos.y + row.pos.y + (row.row.size.y - f.layout.height()) / 2.0 + f.layout.ascent;
    let x = span.0 + ((span.1 - span.0 - f.layout.width) / 2.0).max(0.0);
    paint(painter, f, Pos2::new(x, y), color);
}
