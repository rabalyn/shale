//! Typesetting of a parsed formula (EDIT-047): boxes laid out by the rules of TeXbook appendix G
//! with the constants of the font's MATH table. The result is a list of glyphs and rules at
//! positions, independent of any drawing: the screen and the PDF both paint it.

use super::font::{font, MathFont};
use super::parse::{Class, Env, Force, Limits, Node};
use skrifa::raw::tables::math::{MathConstant as K, MathKernCorner};
use skrifa::GlyphId;

/// One thing to draw. Coordinates are in points from the left end and the baseline of the
/// formula, x to the right and y UP.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// a glyph of the math font at its origin, `size` the em size
    Glyph { gid: GlyphId, x: f32, y: f32, size: f32 },
    /// a filled rectangle (fraction bars, roots, overlines), `y` its bottom
    Rule { x: f32, y: f32, w: f32, h: f32 },
}

/// A typeset formula.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub width: f32,
    /// above the baseline
    pub ascent: f32,
    /// below the baseline
    pub descent: f32,
    pub items: Vec<Item>,
}

impl Layout {
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// Typeset `node` with an em of `size` points, in display style or text style.
pub fn layout(node: &Node, size: f32, display: bool) -> Result<Layout, String> {
    let l = Ctx { f: font(), base: size.clamp(1.0, 400.0) };
    let st = Style { level: if display { 0 } else { 1 }, cramped: false };
    let b = match node {
        Node::Group(nodes) => l.row(nodes, st)?,
        other => l.node(other, st)?,
    };
    Ok(Layout { width: b.w, ascent: b.h, descent: b.d, items: b.items })
}

/// The four sizes of TeX (display, text, script, scriptscript) and whether the style is cramped.
#[derive(Clone, Copy)]
struct Style {
    level: u8,
    cramped: bool,
}

impl Style {
    fn cramped(self) -> Style {
        Style { cramped: true, ..self }
    }
    fn sup(self) -> Style {
        Style { level: if self.level <= 1 { 2 } else { 3 }, cramped: self.cramped }
    }
    fn sub(self) -> Style {
        Style { level: if self.level <= 1 { 2 } else { 3 }, cramped: true }
    }
    fn num(self) -> Style {
        Style { level: (self.level + 1).min(3), cramped: self.cramped }
    }
    fn den(self) -> Style {
        Style { level: (self.level + 1).min(3), cramped: true }
    }
    fn script(self) -> bool {
        self.level >= 2
    }
}

/// The kinds of atoms for the spaces between them (TeXbook chapter 18).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cls {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

impl From<Class> for Cls {
    fn from(c: Class) -> Cls {
        match c {
            Class::Ord => Cls::Ord,
            Class::Bin => Cls::Bin,
            Class::Rel => Cls::Rel,
            Class::Open => Cls::Open,
            Class::Close => Cls::Close,
            Class::Punct => Cls::Punct,
        }
    }
}

/// The space between two atoms in mu (1/18 em): 3 thin, 4 medium, 5 thick; the second is what is
/// left in script styles.
fn atom_space(l: Cls, r: Cls, script: bool) -> f32 {
    // rows: left class, columns: right class, in the order Ord Op Bin Rel Open Close Punct Inner;
    // a digit is the space, 'c' after it (as +10) means "not in script styles"
    const T: [[u8; 8]; 8] = [
        [0, 1, 12, 13, 0, 0, 0, 11],
        [1, 1, 0, 13, 0, 0, 0, 11],
        [12, 12, 0, 0, 12, 0, 0, 12],
        [13, 13, 0, 0, 13, 0, 0, 13],
        [0, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 12, 13, 0, 0, 0, 11],
        [11, 11, 0, 11, 11, 11, 11, 11],
        [11, 1, 12, 13, 11, 0, 11, 11],
    ];
    let idx = |c: Cls| c as usize;
    let v = T[idx(l)][idx(r)];
    let (amount, tight) = (v % 10, v >= 10);
    if script && tight {
        return 0.0;
    }
    [0.0, 3.0, 4.0, 5.0][amount as usize]
}

/// A typeset piece: its extent around the baseline and what to draw.
struct Bx {
    w: f32,
    h: f32,
    d: f32,
    items: Vec<Item>,
    /// the italic correction that `w` already includes
    italic: f32,
    /// a single glyph (scripts hang from its edges, not from its height)
    glyph: bool,
    /// the glyph that scripts tuck against (its kerning table)
    gid: Option<GlyphId>,
    /// where an accent sits over it, from its left edge
    attach: Option<f32>,
}

impl Bx {
    fn empty() -> Bx {
        Bx { w: 0.0, h: 0.0, d: 0.0, items: Vec::new(), italic: 0.0, glyph: false, gid: None, attach: None }
    }
}

fn put(into: &mut Vec<Item>, from: &[Item], dx: f32, dy: f32) {
    for it in from {
        into.push(match it {
            Item::Glyph { gid, x, y, size } => Item::Glyph { gid: *gid, x: x + dx, y: y + dy, size: *size },
            Item::Rule { x, y, w, h } => Item::Rule { x: x + dx, y: y + dy, w: *w, h: *h },
        });
    }
}

/// A glyph, or a stack of glyphs, drawn at an em size, with the ink around it (y up).
struct Stretched {
    items: Vec<Item>,
    y0: f32,
    y1: f32,
    w: f32,
}

struct Ctx {
    f: &'static MathFont,
    /// the em of the text style, in points
    base: f32,
}

impl Ctx {
    /// The em size of a style in points.
    fn em(&self, st: Style) -> f32 {
        let pct = match st.level {
            0 | 1 => 100.0,
            2 => self.f.constant(K::ScriptPercentScaleDown),
            _ => self.f.constant(K::ScriptScriptPercentScaleDown),
        };
        self.base * if pct > 0.0 { pct / 100.0 } else { 1.0 }
    }

    /// Points per font unit in a style.
    fn unit(&self, st: Style) -> f32 {
        self.em(st) / self.f.upem()
    }

    /// A constant of the font in points in a style.
    fn k(&self, c: K, st: Style) -> f32 {
        self.f.constant(c) * self.unit(st)
    }

    fn glyph_box(&self, ch: char, st: Style) -> Result<Bx, String> {
        let gid = self.f.glyph(ch).ok_or_else(|| format!("the math font has no glyph for {ch} (U+{:04X})", ch as u32))?;
        let u = self.unit(st);
        let b = self.f.bounds(gid);
        let italic = self.f.italic(gid) * u;
        Ok(Bx {
            w: self.f.advance(gid) * u + italic,
            h: (b.y1 * u).max(0.0),
            d: (-b.y0 * u).max(0.0),
            items: vec![Item::Glyph { gid, x: 0.0, y: 0.0, size: self.em(st) }],
            italic,
            glyph: true,
            gid: Some(gid),
            attach: Some(self.f.top_accent(gid).map_or_else(|| (b.x0 + b.x1) / 2.0 * u, |a| a * u)),
        })
    }

    fn node(&self, n: &Node, st: Style) -> Result<Bx, String> {
        match n {
            Node::Char(c, _) => self.glyph_box(*c, st),
            Node::Text(s) => self.text(s, st),
            Node::Group(nodes) => self.row(nodes, st),
            Node::Scripts { base, sub, sup } => self.scripts(base, sub.as_deref(), sup.as_deref(), st),
            Node::Frac { num, den, bar, force } => self.frac(num, den, *bar, *force, st),
            Node::Sqrt { index, body } => self.sqrt(index.as_deref(), body, st),
            Node::Op { symbol, name, .. } => self.op(*symbol, name, st),
            Node::Delimited { left, right, body } => {
                let inner = self.row(body, st)?;
                self.wrap(*left, *right, inner, st)
            }
            Node::Accent { mark, body } => self.accent(*mark, body, st),
            Node::Line { over, body } => self.line(*over, body, st),
            Node::Space(em) => Ok(Bx { w: em * self.em(st), ..Bx::empty() }),
            Node::Matrix { env, rows } => self.matrix(*env, rows, st),
        }
    }

    fn text(&self, s: &str, st: Style) -> Result<Bx, String> {
        let mut out = Bx::empty();
        for c in s.chars() {
            if c == ' ' {
                out.w += self.f.glyph(' ').map_or(0.25 * self.em(st), |g| self.f.advance(g) * self.unit(st));
                continue;
            }
            let g = self.glyph_box(c, st)?;
            put(&mut out.items, &g.items, out.w, 0.0);
            out.w += g.w;
            out.h = out.h.max(g.h);
            out.d = out.d.max(g.d);
        }
        Ok(out)
    }

    /// The atom class of a node, `None` for a space (which is no atom).
    fn cls(n: &Node) -> Option<Cls> {
        Some(match n {
            Node::Char(_, c) => (*c).into(),
            Node::Scripts { base, .. } => return Self::cls(base),
            Node::Op { .. } => Cls::Op,
            Node::Frac { .. } | Node::Delimited { .. } => Cls::Inner,
            Node::Space(_) => return None,
            _ => Cls::Ord,
        })
    }

    /// A row of atoms with the spaces between them.
    fn row(&self, nodes: &[Node], st: Style) -> Result<Bx, String> {
        let mut classes: Vec<Option<Cls>> = nodes.iter().map(Self::cls).collect();
        // a binary operator that has no left operand or no right one is an ordinary symbol
        let mut prev: Option<usize> = None;
        for i in 0..classes.len() {
            let Some(c) = classes[i] else { continue };
            if c == Cls::Bin && prev.is_none_or(|p| matches!(classes[p], Some(Cls::Bin | Cls::Op | Cls::Rel | Cls::Open | Cls::Punct))) {
                classes[i] = Some(Cls::Ord);
            } else if matches!(c, Cls::Rel | Cls::Close | Cls::Punct) {
                if let Some(p) = prev.filter(|&p| classes[p] == Some(Cls::Bin)) {
                    classes[p] = Some(Cls::Ord);
                }
            }
            prev = Some(i);
        }
        if let Some(p) = prev.filter(|&p| classes[p] == Some(Cls::Bin)) {
            classes[p] = Some(Cls::Ord);
        }
        let mut out = Bx::empty();
        let mut last: Option<Cls> = None;
        for (n, c) in nodes.iter().zip(&classes) {
            if let (Some(l), Some(r)) = (last, c) {
                out.w += atom_space(l, *r, st.script()) / 18.0 * self.em(st);
            }
            let b = self.node(n, st)?;
            put(&mut out.items, &b.items, out.w, 0.0);
            out.w += b.w;
            out.h = out.h.max(b.h);
            out.d = out.d.max(b.d);
            if nodes.len() == 1 {
                out.glyph = b.glyph;
                out.gid = b.gid;
                out.italic = b.italic;
                out.attach = b.attach;
            }
            if c.is_some() {
                last = *c;
            }
        }
        Ok(out)
    }

    fn scripts(&self, base: &Node, sub: Option<&Node>, sup: Option<&Node>, st: Style) -> Result<Bx, String> {
        if let Node::Op { limits, wants_limits, .. } = base {
            let above = match limits {
                Limits::Yes => true,
                Limits::No => false,
                Limits::Default => *wants_limits && st.level == 0,
            };
            if above {
                return self.limits(base, sub, sup, st);
            }
        }
        let b = self.node(base, st)?;
        let sup_b = sup.map(|n| self.node(n, st.sup())).transpose()?;
        let sub_b = sub.map(|n| self.node(n, st.sub())).transpose()?;
        // the baseline drops of the script's own size, the others of the base's (TeXbook rule 18)
        let (mut u, mut v) = (0.0f32, 0.0f32);
        if !b.glyph {
            u = b.h - self.k(K::SuperscriptBaselineDropMax, st.sup());
            v = b.d + self.k(K::SubscriptBaselineDropMin, st.sub());
        }
        let shift_up = self.k(if st.cramped { K::SuperscriptShiftUpCramped } else { K::SuperscriptShiftUp }, st);
        let mut x_sup = b.w;
        let mut x_sub = b.w - b.italic;
        match (&sup_b, &sub_b) {
            (None, Some(s)) => {
                v = v.max(self.k(K::SubscriptShiftDown, st)).max(s.h - self.k(K::SubscriptTopMax, st));
            }
            (Some(p), None) => {
                u = u.max(shift_up).max(p.d + self.k(K::SuperscriptBottomMin, st));
            }
            (Some(p), Some(s)) => {
                u = u.max(shift_up).max(p.d + self.k(K::SuperscriptBottomMin, st));
                v = v.max(self.k(K::SubscriptShiftDown, st));
                let gap = (u - p.d) - (s.h - v);
                let min_gap = self.k(K::SubSuperscriptGapMin, st);
                if gap < min_gap {
                    v += min_gap - gap;
                    let psi = self.k(K::SuperscriptBottomMaxWithSubscript, st) - (u - p.d);
                    if psi > 0.0 {
                        u += psi;
                        v -= psi;
                    }
                }
            }
            (None, None) => {}
        }
        // the scripts tuck against the corners of a glyph, as its kerning table says
        if let Some(g) = b.gid {
            let unit = self.unit(st);
            let at = |corner: MathKernCorner, lo: f32, hi: f32| self.f.kern(g, corner, lo / unit).min(self.f.kern(g, corner, hi / unit)) * unit;
            if let Some(p) = &sup_b {
                x_sup += at(MathKernCorner::TopRight, u - p.d, u + p.h);
            }
            if let Some(s) = &sub_b {
                x_sub += at(MathKernCorner::BottomRight, -v - s.d, -v + s.h);
            }
        }
        let after = self.k(K::SpaceAfterScript, st);
        let mut out = Bx { w: b.w, h: b.h, d: b.d, items: b.items.clone(), ..Bx::empty() };
        if let Some(p) = &sup_b {
            put(&mut out.items, &p.items, x_sup, u);
            out.w = out.w.max(x_sup + p.w + after);
            out.h = out.h.max(u + p.h);
        }
        if let Some(s) = &sub_b {
            put(&mut out.items, &s.items, x_sub, -v);
            out.w = out.w.max(x_sub + s.w + after);
            out.d = out.d.max(v + s.d);
        }
        Ok(out)
    }

    /// A big operator with its limits above and below, all centred (TeXbook rule 13a).
    fn limits(&self, op: &Node, sub: Option<&Node>, sup: Option<&Node>, st: Style) -> Result<Bx, String> {
        let b = self.node(op, st)?;
        let sup_b = sup.map(|n| self.node(n, st.sup())).transpose()?;
        let sub_b = sub.map(|n| self.node(n, st.sub())).transpose()?;
        let body_w = b.w - b.italic;
        let wide = body_w.max(sup_b.as_ref().map_or(0.0, |s| s.w)).max(sub_b.as_ref().map_or(0.0, |s| s.w));
        let mut out = Bx { w: wide, h: b.h, d: b.d, ..Bx::empty() };
        put(&mut out.items, &b.items, (wide - body_w) / 2.0, 0.0);
        if let Some(p) = &sup_b {
            let y = b.h + self.k(K::UpperLimitBaselineRiseMin, st).max(self.k(K::UpperLimitGapMin, st) + p.d);
            put(&mut out.items, &p.items, (wide - p.w) / 2.0 + b.italic / 2.0, y);
            out.h = y + p.h;
        }
        if let Some(s) = &sub_b {
            let y = b.d + self.k(K::LowerLimitBaselineDropMin, st).max(self.k(K::LowerLimitGapMin, st) + s.h);
            put(&mut out.items, &s.items, (wide - s.w) / 2.0 - b.italic / 2.0, -y);
            out.d = y + s.d;
        }
        Ok(out)
    }

    /// A big operator: the symbol (larger in display style, centred on the axis) or the name.
    fn op(&self, symbol: Option<char>, name: &str, st: Style) -> Result<Bx, String> {
        let Some(c) = symbol else {
            return self.text(name, st);
        };
        let min = if st.level == 0 { self.k(K::DisplayOperatorMinHeight, st) } else { 0.0 };
        let s = self.stretch_vertical(c, min, st)?;
        let a = self.k(K::AxisHeight, st);
        let dy = a - (s.y0 + s.y1) / 2.0;
        let italic = match s.items.as_slice() {
            [Item::Glyph { gid, .. }] => self.f.italic(*gid) * self.unit(st),
            _ => 0.0,
        };
        let gid = match s.items.as_slice() {
            [Item::Glyph { gid, .. }] => Some(*gid),
            _ => None,
        };
        let mut out = Bx { w: s.w + italic, h: s.y1 + dy, d: -(s.y0 + dy), italic, gid, ..Bx::empty() };
        put(&mut out.items, &s.items, 0.0, dy);
        Ok(out)
    }

    /// `c` or the smallest of its larger variants that is at least `min` tall, or a stack of parts.
    fn stretch_vertical(&self, c: char, min: f32, st: Style) -> Result<Stretched, String> {
        let gid = self.f.glyph(c).ok_or_else(|| format!("the math font has no glyph for {c} (U+{:04X})", c as u32))?;
        let u = self.unit(st);
        let size = self.em(st);
        let single = |g: GlyphId| {
            let b = self.f.bounds(g);
            Stretched { items: vec![Item::Glyph { gid: g, x: 0.0, y: 0.0, size }], y0: b.y0 * u, y1: b.y1 * u, w: self.f.advance(g) * u }
        };
        let own = single(gid);
        if own.y1 - own.y0 >= min {
            return Ok(own);
        }
        let Some(cons) = self.f.construction(gid, true) else { return Ok(own) };
        if let Some((g, _)) = cons.variants.iter().find(|(g, m)| {
            let b = self.f.bounds(*g);
            m * u >= min && (b.y1 - b.y0) * u >= min * 0.98
        }) {
            return Ok(single(*g));
        }
        if cons.parts.is_empty() {
            return Ok(cons.variants.last().map_or(own, |(g, _)| single(*g)));
        }
        // an assembly: repeat the extenders until the parts are long enough, then overlap them
        // evenly to the size that is wanted
        let min_overlap = self.f.min_overlap() * u;
        let seq_for = |repeats: usize| {
            let mut seq = Vec::new();
            for p in &cons.parts {
                for _ in 0..if p.extender { repeats } else { 1 } {
                    seq.push(*p);
                }
            }
            seq
        };
        let mut seq = seq_for(0);
        for r in 1..=64 {
            let total: f32 = seq.iter().map(|p| p.advance * u).sum::<f32>() - seq.len().saturating_sub(1) as f32 * min_overlap;
            if !seq.is_empty() && total >= min {
                break;
            }
            seq = seq_for(r);
        }
        let sum: f32 = seq.iter().map(|p| p.advance * u).sum();
        let joints = seq.len().saturating_sub(1) as f32;
        let max_overlap = seq.windows(2).map(|w| w[0].end.min(w[1].start) * u).fold(f32::MAX, f32::min);
        let overlap = if joints > 0.0 { ((sum - min) / joints).clamp(min_overlap, max_overlap.max(min_overlap)) } else { 0.0 };
        let mut items = Vec::new();
        let (mut bottom, mut y0, mut y1, mut w) = (0.0f32, f32::MAX, f32::MIN, 0.0f32);
        for p in &seq {
            let b = self.f.bounds(p.gid);
            // the glyph sits with the foot of its ink at the foot of its part
            let origin = bottom - b.y0 * u;
            items.push(Item::Glyph { gid: p.gid, x: 0.0, y: origin, size });
            y0 = y0.min(bottom);
            y1 = y1.max(bottom + (b.y1 - b.y0) * u);
            w = w.max(self.f.advance(p.gid) * u);
            bottom += p.advance * u - overlap;
        }
        Ok(Stretched { items, y0, y1, w })
    }

    /// A delimiter that covers `total` points, centred on the axis.
    fn delimiter(&self, c: char, total: f32, st: Style) -> Result<Bx, String> {
        let s = self.stretch_vertical(c, total, st)?;
        let a = self.k(K::AxisHeight, st);
        let dy = a - (s.y0 + s.y1) / 2.0;
        let mut out = Bx { w: s.w, h: s.y1 + dy, d: -(s.y0 + dy), ..Bx::empty() };
        put(&mut out.items, &s.items, 0.0, dy);
        Ok(out)
    }

    /// `inner` between delimiters that grow with it (TeXbook rule 19).
    fn wrap(&self, left: Option<char>, right: Option<char>, inner: Bx, st: Style) -> Result<Bx, String> {
        let a = self.k(K::AxisHeight, st);
        let delta = (inner.h - a).max(inner.d + a);
        let total = (delta * 2.0 * 0.901).max(delta * 2.0 - 0.5 * self.em(st));
        let null = 0.08 * self.em(st);
        let l = left.map(|c| self.delimiter(c, total, st)).transpose()?;
        let r = right.map(|c| self.delimiter(c, total, st)).transpose()?;
        let mut out = Bx::empty();
        let mut x = 0.0;
        match &l {
            Some(b) => {
                put(&mut out.items, &b.items, 0.0, 0.0);
                x = b.w;
                out.h = b.h;
                out.d = b.d;
            }
            None => x += null,
        }
        put(&mut out.items, &inner.items, x, 0.0);
        x += inner.w;
        out.h = out.h.max(inner.h);
        out.d = out.d.max(inner.d);
        match &r {
            Some(b) => {
                put(&mut out.items, &b.items, x, 0.0);
                x += b.w;
                out.h = out.h.max(b.h);
                out.d = out.d.max(b.d);
            }
            None => x += null,
        }
        out.w = x;
        Ok(out)
    }

    fn frac(&self, num: &Node, den: &Node, bar: bool, force: Option<Force>, st: Style) -> Result<Bx, String> {
        let fst = match force {
            Some(Force::Display) => Style { level: 0, cramped: st.cramped },
            Some(Force::Text) => Style { level: 1, cramped: st.cramped },
            None => st,
        };
        let n = self.node(num, fst.num())?;
        let d = self.node(den, fst.den())?;
        let display = fst.level == 0;
        let a = self.k(K::AxisHeight, fst);
        let (mut u, mut v, t);
        if bar {
            t = self.k(K::FractionRuleThickness, fst);
            let (up, down) = if display { (K::FractionNumeratorDisplayStyleShiftUp, K::FractionDenominatorDisplayStyleShiftDown) } else { (K::FractionNumeratorShiftUp, K::FractionDenominatorShiftDown) };
            let (gn, gd) = if display { (K::FractionNumDisplayStyleGapMin, K::FractionDenomDisplayStyleGapMin) } else { (K::FractionNumeratorGapMin, K::FractionDenominatorGapMin) };
            u = self.k(up, fst).max(a + t / 2.0 + self.k(gn, fst) + n.d);
            v = self.k(down, fst).max(d.h + self.k(gd, fst) + t / 2.0 - a);
        } else {
            t = 0.0;
            let (up, down, gap) = if display { (K::StackTopDisplayStyleShiftUp, K::StackBottomDisplayStyleShiftDown, K::StackDisplayStyleGapMin) } else { (K::StackTopShiftUp, K::StackBottomShiftDown, K::StackGapMin) };
            u = self.k(up, fst);
            v = self.k(down, fst);
            let have = (u - n.d) - (d.h - v);
            let want = self.k(gap, fst);
            if have < want {
                u += (want - have) / 2.0;
                v += (want - have) / 2.0;
            }
        }
        let wide = n.w.max(d.w);
        let null = 0.08 * self.em(st);
        let mut out = Bx { w: wide + 2.0 * null, h: u + n.h, d: v + d.d, ..Bx::empty() };
        put(&mut out.items, &n.items, null + (wide - n.w) / 2.0, u);
        put(&mut out.items, &d.items, null + (wide - d.w) / 2.0, -v);
        if bar {
            out.items.push(Item::Rule { x: null, y: a - t / 2.0, w: wide, h: t });
        }
        Ok(out)
    }

    fn sqrt(&self, index: Option<&Node>, body: &Node, st: Style) -> Result<Bx, String> {
        let b = self.node(body, st.cramped())?;
        let gap = self.k(if st.level == 0 { K::RadicalDisplayStyleVerticalGap } else { K::RadicalVerticalGap }, st);
        let t = self.k(K::RadicalRuleThickness, st);
        let need = b.h + b.d + gap + t;
        let r = self.stretch_vertical('\u{221a}', need, st)?;
        let extra = ((r.y1 - r.y0) - need).max(0.0);
        let gap = gap + extra / 2.0;
        let shift = b.h + gap + t - r.y1;
        let (mut deg, mut deg_at) = (None, 0.0);
        if let Some(i) = index {
            let dg = self.node(i, Style { level: 3, cramped: false })?;
            let (kb, ka) = (self.k(K::RadicalKernBeforeDegree, st), self.k(K::RadicalKernAfterDegree, st));
            let lead = (-kb).max(-(kb + dg.w + ka)).max(0.0);
            let pct = self.f.constant(K::RadicalDegreeBottomRaisePercent) / 100.0;
            deg_at = (r.y0 + shift) + pct * (r.y1 - r.y0) + dg.d;
            deg = Some((dg, kb + lead));
        }
        let rx = match &deg {
            Some((dg, at)) => at + dg.w + self.k(K::RadicalKernAfterDegree, st),
            None => 0.0,
        };
        let mut out = Bx { w: rx + r.w + b.w, h: b.h + gap + t + self.k(K::RadicalExtraAscender, st), d: b.d.max(-(r.y0 + shift)), ..Bx::empty() };
        put(&mut out.items, &r.items, rx, shift);
        out.items.push(Item::Rule { x: rx + r.w, y: b.h + gap, w: b.w, h: t });
        put(&mut out.items, &b.items, rx + r.w, 0.0);
        if let Some((dg, at)) = deg {
            put(&mut out.items, &dg.items, at, deg_at);
            out.h = out.h.max(deg_at + dg.h);
        }
        Ok(out)
    }

    fn accent(&self, mark: char, body: &Node, st: Style) -> Result<Bx, String> {
        let b = self.node(body, st.cramped())?;
        let gid = self.f.glyph(mark).ok_or_else(|| format!("the math font has no glyph for {mark} (U+{:04X})", mark as u32))?;
        let u = self.unit(st);
        let bb = self.f.bounds(gid);
        let attach_base = b.attach.unwrap_or(b.w / 2.0);
        let attach_mark = self.f.top_accent(gid).unwrap_or_else(|| (bb.x0 + bb.x1) / 2.0) * u;
        let dy = (b.h - self.k(K::AccentBaseHeight, st)).max(0.0);
        let mut out = Bx { w: b.w, h: b.h.max(dy + bb.y1 * u), d: b.d, ..Bx::empty() };
        put(&mut out.items, &b.items, 0.0, 0.0);
        out.items.push(Item::Glyph { gid, x: attach_base - attach_mark, y: dy, size: self.em(st) });
        Ok(out)
    }

    fn line(&self, over: bool, body: &Node, st: Style) -> Result<Bx, String> {
        let b = self.node(body, st.cramped())?;
        let mut out = Bx { w: b.w, h: b.h, d: b.d, ..Bx::empty() };
        put(&mut out.items, &b.items, 0.0, 0.0);
        if over {
            let (gap, t) = (self.k(K::OverbarVerticalGap, st), self.k(K::OverbarRuleThickness, st));
            out.items.push(Item::Rule { x: 0.0, y: b.h + gap, w: b.w, h: t });
            out.h = b.h + gap + t + self.k(K::OverbarExtraAscender, st);
        } else {
            let (gap, t) = (self.k(K::UnderbarVerticalGap, st), self.k(K::UnderbarRuleThickness, st));
            out.items.push(Item::Rule { x: 0.0, y: -(b.d + gap + t), w: b.w, h: t });
            out.d = b.d + gap + t + self.k(K::UnderbarExtraDescender, st);
        }
        Ok(out)
    }

    fn matrix(&self, env: Env, rows: &[Vec<Node>], st: Style) -> Result<Bx, String> {
        // matrices are set in text style, `cases` and `aligned` keep the style they are in
        let cell_st = match env {
            Env::Cases | Env::Aligned => st,
            _ => Style { level: st.level.max(1), cramped: false },
        };
        let em = self.em(cell_st);
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cells: Vec<Vec<Bx>> = Vec::new();
        for row in rows {
            let mut line = Vec::new();
            for (c, cell) in row.iter().enumerate() {
                let b = match (cell, env, c % 2) {
                    // the second column of an `aligned` starts with an empty atom: `=` gets its spaces
                    (Node::Group(g), Env::Aligned, 1) => {
                        let mut with = vec![Node::Group(Vec::new())];
                        with.extend(g.iter().cloned());
                        self.row(&with, cell_st)?
                    }
                    _ => self.node(cell, cell_st)?,
                };
                line.push(b);
            }
            cells.push(line);
        }
        let mut widths = vec![0.0f32; columns];
        for line in &cells {
            for (c, b) in line.iter().enumerate() {
                widths[c] = widths[c].max(b.w);
            }
        }
        let gap = |c: usize| match env {
            Env::Aligned => {
                if c.is_multiple_of(2) {
                    0.0
                } else {
                    2.0 * em
                }
            }
            _ => em,
        };
        let row_gap = match env {
            Env::Aligned => 0.3 * em,
            Env::Cases => 0.2 * em,
            _ => 0.0,
        };
        let (strut_h, strut_d) = (0.84 * em, 0.36 * em);
        let heights: Vec<(f32, f32)> = cells.iter().map(|line| line.iter().fold((strut_h, strut_d), |(h, d), b| (h.max(b.h), d.max(b.d)))).collect();
        let total: f32 = heights.iter().map(|(h, d)| h + d).sum::<f32>() + row_gap * heights.len().saturating_sub(1) as f32;
        let a = self.k(K::AxisHeight, cell_st);
        let top = a + total / 2.0;
        let mut inner = Bx { h: top, d: total - top, ..Bx::empty() };
        let mut y = top;
        for (line, (h, d)) in cells.iter().zip(&heights) {
            let base = y - h;
            let mut x = 0.0;
            for (c, b) in line.iter().enumerate() {
                let slack = widths[c] - b.w;
                let off = match env {
                    Env::Matrix | Env::PMatrix | Env::BMatrix | Env::VMatrix => slack / 2.0,
                    Env::Cases => 0.0,
                    Env::Aligned => {
                        if c.is_multiple_of(2) {
                            slack
                        } else {
                            0.0
                        }
                    }
                };
                put(&mut inner.items, &b.items, x + off, base);
                x += widths[c] + if c + 1 < columns { gap(c) } else { 0.0 };
            }
            inner.w = inner.w.max(x);
            y = base - d - row_gap;
        }
        let (l, r) = match env {
            Env::PMatrix => (Some('('), Some(')')),
            Env::BMatrix => (Some('['), Some(']')),
            Env::VMatrix => (Some('|'), Some('|')),
            Env::Cases => (Some('{'), None),
            _ => (None, None),
        };
        if l.is_none() && r.is_none() {
            return Ok(inner);
        }
        self.wrap(l, r, inner, st)
    }
}
