//! The bundled math font, STIX Two Math, and its OpenType MATH table (EDIT-047): the constants,
//! italic corrections, accent attachments and the growing variants and assemblies of delimiters,
//! radicals and big operators. The font is parsed when the first formula is laid out.

use skrifa::charmap::Charmap;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::tables::math::{Math, MathConstant, MathKernCorner, StretchAxis};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider, OutlineGlyphCollection};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The font file (SIL OFL, `assets/fonts/STIXTwoMath-LICENSE.txt`).
pub const DATA: &[u8] = include_bytes!("../../assets/fonts/STIXTwoMath-Regular.otf");

/// The ink of a glyph in font units, y up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// One part of an assembled glyph (a bracket made of a top, an extender and a bottom).
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub gid: GlyphId,
    pub start: f32,
    pub end: f32,
    pub advance: f32,
    pub extender: bool,
}

/// How a glyph grows: ready-made larger variants, smallest first, and the parts to build any size.
#[derive(Clone, Debug, Default)]
pub struct Construction {
    pub variants: Vec<(GlyphId, f32)>,
    /// bottom to top (or left to right)
    pub parts: Vec<Part>,
}

pub struct MathFont {
    math: Math<'static>,
    charmap: Charmap<'static>,
    advances: GlyphMetrics<'static>,
    outlines: OutlineGlyphCollection<'static>,
    upem: f32,
    constants: Vec<i32>,
    bounds: Mutex<HashMap<GlyphId, Bounds>>,
}

/// The font, parsed on first use.
pub fn font() -> &'static MathFont {
    static FONT: OnceLock<MathFont> = OnceLock::new();
    FONT.get_or_init(|| MathFont::parse(DATA).expect("the bundled math font"))
}

struct BoundsPen(Option<Bounds>);

impl BoundsPen {
    fn add(&mut self, x: f32, y: f32) {
        let b = self.0.get_or_insert(Bounds { x0: x, y0: y, x1: x, y1: y });
        b.x0 = b.x0.min(x);
        b.y0 = b.y0.min(y);
        b.x1 = b.x1.max(x);
        b.y1 = b.y1.max(y);
    }
}

impl OutlinePen for BoundsPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.add(x1, y1);
        self.add(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.add(x1, y1);
        self.add(x2, y2);
        self.add(x, y);
    }
    fn close(&mut self) {}
}

impl MathFont {
    fn parse(data: &'static [u8]) -> Option<MathFont> {
        let font = FontRef::new(data).ok()?;
        let math = font.math().ok()?;
        let consts = math.math_constants().ok()?;
        let constants = MathConstant::ALL.iter().map(|c| consts.constant(*c)).collect();
        let upem = font.metrics(Size::unscaled(), LocationRef::default()).units_per_em as f32;
        Some(MathFont { math, charmap: font.charmap(), advances: font.glyph_metrics(Size::unscaled(), LocationRef::default()), outlines: font.outline_glyphs(), upem, constants, bounds: Mutex::new(HashMap::new()) })
    }

    pub fn upem(&self) -> f32 {
        self.upem
    }

    pub fn glyph(&self, c: char) -> Option<GlyphId> {
        self.charmap.map(c).filter(|g| g.to_u32() != 0)
    }

    pub fn advance(&self, g: GlyphId) -> f32 {
        self.advances.advance_width(g).unwrap_or(0.0)
    }

    /// The constant `c` in font units.
    pub fn constant(&self, c: MathConstant) -> f32 {
        self.constants.get(c as usize).copied().unwrap_or(0) as f32
    }

    pub fn bounds(&self, g: GlyphId) -> Bounds {
        if let Some(b) = self.bounds.lock().ok().and_then(|m| m.get(&g).copied()) {
            return b;
        }
        let mut pen = BoundsPen(None);
        self.draw(g, &mut pen);
        let b = pen.0.unwrap_or_default();
        if let Ok(mut m) = self.bounds.lock() {
            m.insert(g, b);
        }
        b
    }

    /// Draw the outline of `g` in font units (y up).
    pub fn draw(&self, g: GlyphId, pen: &mut impl OutlinePen) {
        if let Some(o) = self.outlines.get(g) {
            // a glyph that cannot be drawn is an empty one
            let _ = o.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), pen);
        }
    }

    /// The italic correction of `g`: how far its ink leans past its advance.
    pub fn italic(&self, g: GlyphId) -> f32 {
        let info = self.math.math_glyph_info().ok();
        info.and_then(|i| i.math_italics_correction_info()).and_then(|r| r.ok()).and_then(|i| i.correction(g)).unwrap_or(0) as f32
    }

    /// How far a script at `height` (font units above the baseline) moves in or out at a corner of
    /// `g` (the kerning of the MATH table; negative: toward the glyph).
    pub fn kern(&self, g: GlyphId, corner: MathKernCorner, height: f32) -> f32 {
        let info = self.math.math_glyph_info().ok().and_then(|i| i.math_kern_info()).and_then(|r| r.ok());
        info.and_then(|i| i.kern(g, corner)).and_then(|k| k.kerning(height.round() as i32)).unwrap_or(0) as f32
    }

    /// Where an accent sits over `g`, from its left edge.
    pub fn top_accent(&self, g: GlyphId) -> Option<f32> {
        let info = self.math.math_glyph_info().ok()?;
        info.math_top_accent_attachment()?.ok()?.attachment(g).map(|v| v as f32)
    }

    /// The larger variants of `g` and the assembly, along the axis.
    pub fn construction(&self, g: GlyphId, vertical: bool) -> Option<Construction> {
        let variants = self.math.math_variants().ok()?;
        let axis = if vertical { StretchAxis::Vertical } else { StretchAxis::Horizontal };
        let c = variants.glyph_construction(g, axis)?;
        let list = c.math_glyph_variant_records().iter().map(|r| (GlyphId::from(r.variant_glyph()), r.advance_measurement().to_u16() as f32)).collect();
        let parts = match c.glyph_assembly().and_then(|a| a.ok()) {
            Some(a) => a
                .part_records()
                .iter()
                .map(|p| Part {
                    gid: GlyphId::from(p.glyph_id()),
                    start: p.start_connector_length().to_u16() as f32,
                    end: p.end_connector_length().to_u16() as f32,
                    advance: p.full_advance().to_u16() as f32,
                    extender: p.part_flags().contains(skrifa::raw::tables::math::PartFlags::EXTENDER_FLAG),
                })
                .collect(),
            None => Vec::new(),
        };
        Some(Construction { variants: list, parts })
    }

    /// The least overlap of the parts of an assembly, in font units.
    pub fn min_overlap(&self) -> f32 {
        self.math.math_variants().map_or(0.0, |v| v.min_connector_overlap().to_u16() as f32)
    }
}
