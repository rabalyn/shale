//! A typeset formula as a coverage mask: the outlines of its glyphs and its rules, filled with
//! exact area coverage (the accumulation method of font-rs), at the pixel size of the screen so the
//! result is sharp. The mask is the alpha of the picture the screen draws in the text colour.

use super::font::font;
use super::layout::{Item, Layout};
use skrifa::outline::OutlinePen;

/// Margin around the formula in pixels, for ink that reaches past its box.
pub const PAD: usize = 2;

/// The most pixels a mask has: a huge formula is drawn at a smaller scale than the screen's.
const MAX_PIXELS: f32 = 6_000_000.0;

pub struct Mask {
    /// the pixels per point it was made at: the screen's, or less for a huge formula
    pub scale: f32,
    pub width: usize,
    pub height: usize,
    /// the coverage, 0 to 255, row by row
    pub alpha: Vec<u8>,
    /// where the baseline is, in pixels from the top of the mask
    pub baseline: f32,
}

/// Accumulates the signed area of the edges; the sum along a row is the coverage.
struct Canvas {
    w: usize,
    h: usize,
    stride: usize,
    acc: Vec<f32>,
}

impl Canvas {
    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if p0.1 == p1.1 {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 { (1.0f32, p0, p1) } else { (-1.0, p1, p0) };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let mut y_from = p0.1;
        if y_from < 0.0 {
            x -= y_from * dxdy;
            y_from = 0.0;
        }
        let last = (p1.1.ceil().max(0.0) as usize).min(self.h);
        let wmax = self.w as f32;
        for y in (y_from as usize)..last {
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(y_from);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (a, b) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let (a, b) = (a.clamp(0.0, wmax), b.clamp(0.0, wmax));
            let row = y * self.stride;
            let xa = a.floor();
            let xai = xa as usize;
            let xb = b.ceil();
            let xbi = xb as usize;
            if xbi <= xai + 1 {
                let xmf = 0.5 * (a + b) - xa;
                self.acc[row + xai] += d - d * xmf;
                self.acc[row + xai + 1] += d * xmf;
            } else {
                let s = 1.0 / (b - a);
                let x0f = a - xa;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = b - xb + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.acc[row + xai] += d * a0;
                if xbi == xai + 2 {
                    self.acc[row + xai + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.acc[row + xai + 1] += d * (a1 - a0);
                    for xi in xai + 2..xbi - 1 {
                        self.acc[row + xi] += d * s;
                    }
                    let a2 = a1 + (xbi - xai - 3) as f32 * s;
                    self.acc[row + xbi - 1] += d * (1.0 - a2 - am);
                }
                self.acc[row + xbi] += d * am;
            }
            x = xnext;
        }
    }
}

/// Feeds an outline to the canvas through a transform from font units to pixels.
struct Pen<'a> {
    canvas: &'a mut Canvas,
    ox: f32,
    oy: f32,
    scale: f32,
    start: (f32, f32),
    at: (f32, f32),
}

impl Pen<'_> {
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + x * self.scale, self.oy - y * self.scale)
    }
    fn to(&mut self, p: (f32, f32)) {
        self.canvas.line(self.at, p);
        self.at = p;
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = self.map(x, y);
        self.at = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.to(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.at, self.map(x1, y1), self.map(x, y));
        let n = steps(&[p0, p1, p2]);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.to((u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.at, self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        let n = steps(&[p0, p1, p2, p3]);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.to((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
        }
    }
    fn close(&mut self) {
        let start = self.start;
        self.to(start);
    }
}

/// How many straight pieces a curve with these control points needs.
fn steps(pts: &[(f32, f32)]) -> usize {
    let len: f32 = pts.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
    ((len / 2.0).sqrt().ceil() as usize).clamp(2, 24)
}

/// The mask of `layout` at `scale` pixels per point.
pub fn rasterise(layout: &Layout, scale: f32) -> Mask {
    let pad = PAD as f32;
    // a formula of thousands of points in both directions would need more memory than it is worth
    let area = ((layout.width * scale).ceil() + 2.0 * pad) * ((layout.height() * scale).ceil() + 2.0 * pad);
    let scale = if area > MAX_PIXELS { scale * (MAX_PIXELS / area).sqrt() } else { scale };
    let w = ((layout.width * scale).ceil().max(1.0) as usize + 2 * PAD).min(8192);
    let baseline = (layout.ascent * scale).ceil() + pad;
    let h = ((baseline + (layout.descent * scale).ceil() + pad).max(1.0) as usize).min(8192);
    let mut canvas = Canvas { w, h, stride: w + 2, acc: vec![0.0; (w + 2) * h] };
    let f = font();
    for item in &layout.items {
        match item {
            Item::Glyph { gid, x, y, size } => {
                let mut pen = Pen { canvas: &mut canvas, ox: pad + x * scale, oy: baseline - y * scale, scale: size / f.upem() * scale, start: (0.0, 0.0), at: (0.0, 0.0) };
                f.draw(*gid, &mut pen);
                pen.close();
            }
            Item::Rule { x, y, w: rw, h: rh } => {
                // at least a pixel thin, so a hairline does not vanish
                let rh = rh.max(1.0 / scale);
                let (x0, x1) = (pad + x * scale, pad + (x + rw) * scale);
                let (y1, y0) = (baseline - y * scale, baseline - (y + rh) * scale);
                canvas.line((x0, y1), (x0, y0));
                canvas.line((x0, y0), (x1, y0));
                canvas.line((x1, y0), (x1, y1));
                canvas.line((x1, y1), (x0, y1));
            }
        }
    }
    let mut alpha = vec![0u8; w * h];
    for y in 0..h {
        let mut sum = 0.0f32;
        for x in 0..w {
            sum += canvas.acc[y * canvas.stride + x];
            alpha[y * w + x] = (sum.abs().min(1.0) * 255.0 + 0.5) as u8;
        }
    }
    Mask { scale, width: w, height: h, alpha, baseline }
}
