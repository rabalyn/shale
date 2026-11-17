//! The TeX subset, its layout and its mask (EDIT-047, EDIT-049): what parses, what does not and
//! why, where the parts of a formula end up relative to each other.

use super::font::font;
use super::layout::{layout, Item, Layout};
use super::parse::{parse, Class, Env, Limits, Node, CAPITALS, FUNCTIONS, GREEK, MAX_CHARS, MAX_DEPTH, SYMBOLS};
use super::raster::rasterise;
use super::typeset;
use skrifa::GlyphId;
use std::sync::Arc;

const SIZE: f32 = 20.0;

fn lay(tex: &str, display: bool) -> Layout {
    match typeset(tex, SIZE, display) {
        Ok(f) => f.layout.clone(),
        Err(e) => panic!("{tex}: {e}"),
    }
}

fn gid(c: char) -> GlyphId {
    font().glyph(c).unwrap_or_else(|| panic!("the font has no {c}"))
}

/// Where the glyphs of `c` are: (left x, baseline y, em size).
fn at(l: &Layout, c: char) -> Vec<(f32, f32, f32)> {
    let g = gid(c);
    l.items
        .iter()
        .filter_map(|i| match i {
            Item::Glyph { gid, x, y, size } if *gid == g => Some((*x, *y, *size)),
            _ => None,
        })
        .collect()
}

fn one(l: &Layout, c: char) -> (f32, f32, f32) {
    let v = at(l, c);
    assert_eq!(v.len(), 1, "{c} should be there once: {v:?}");
    v[0]
}

fn rules(l: &Layout) -> Vec<(f32, f32, f32, f32)> {
    l.items
        .iter()
        .filter_map(|i| match i {
            Item::Rule { x, y, w, h } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

/// The ink of a glyph item: left, right, bottom, top in points.
fn ink(i: &Item) -> Option<(f32, f32, f32, f32)> {
    match i {
        Item::Glyph { gid, x, y, size } => {
            let b = font().bounds(*gid);
            let k = size / font().upem();
            Some((x + b.x0 * k, x + b.x1 * k, y + b.y0 * k, y + b.y1 * k))
        }
        Item::Rule { .. } => None,
    }
}

fn node(tex: &str) -> Vec<Node> {
    match parse(tex) {
        Ok(Node::Group(v)) => v,
        other => panic!("{tex}: {other:?}"),
    }
}

fn why(tex: &str) -> String {
    match parse(tex) {
        Ok(n) => panic!("{tex} should not parse: {n:?}"),
        Err(e) => e.0,
    }
}

// spec: EDIT-047
#[test]
fn letters_are_in_their_alphabet_and_digits_and_operators_upright() {
    assert_eq!(node("x"), [Node::Char('\u{1D465}', Class::Ord)], "math italic x");
    assert_eq!(node("h"), [Node::Char('\u{210E}', Class::Ord)], "the Planck h is the hole in the alphabet");
    assert_eq!(node("A"), [Node::Char('\u{1D434}', Class::Ord)]);
    assert_eq!(node("7"), [Node::Char('7', Class::Ord)], "digits are upright");
    assert_eq!(node("+"), [Node::Char('+', Class::Bin)]);
    assert_eq!(node("-"), [Node::Char('\u{2212}', Class::Bin)], "a real minus");
    assert_eq!(node("="), [Node::Char('=', Class::Rel)]);
    assert_eq!(node(r"\alpha"), [Node::Char('\u{1D6FC}', Class::Ord)], "Greek lower case is italic");
    assert_eq!(node(r"\Gamma"), [Node::Char('\u{393}', Class::Ord)], "Greek capitals upright");
    assert_eq!(node(r"\mathrm{x}"), [Node::Group(vec![Node::Char('x', Class::Ord)])]);
    assert_eq!(node(r"\mathbf{x}"), [Node::Group(vec![Node::Char('\u{1D431}', Class::Ord)])]);
    assert_eq!(node(r"\mathbb{R}"), [Node::Group(vec![Node::Char('\u{211D}', Class::Ord)])], "double-struck R is ℝ");
    assert_eq!(node(r"\mathcal{L}"), [Node::Group(vec![Node::Char('\u{2112}', Class::Ord)])]);
    assert_eq!(node(r"\mathit{x}"), node(r"{x}"));
}

// spec: EDIT-047
#[test]
fn scripts_fractions_roots_and_the_rest_are_read_into_their_nodes() {
    let n = node("x^2_3");
    assert!(matches!(&n[0], Node::Scripts { sub: Some(_), sup: Some(_), .. }), "both scripts in either order: {n:?}");
    assert_eq!(node("x_3^2"), n, "the order does not matter");
    assert!(matches!(&node("x^{a^b}")[0], Node::Scripts { sup: Some(s), .. } if matches!(&**s, Node::Group(g) if matches!(g[0], Node::Scripts { .. }))), "nested scripts");
    assert!(matches!(&node("x'")[0], Node::Scripts { sup: Some(s), .. } if **s == Node::Group(vec![Node::Char('\u{2032}', Class::Ord)])), "a prime is a superscript");
    assert!(matches!(&node(r"\frac12")[0], Node::Frac { bar: true, force: None, .. }), "arguments may be single tokens");
    assert!(matches!(&node(r"\dfrac{a}{b}")[0], Node::Frac { force: Some(super::parse::Force::Display), .. }));
    assert!(matches!(&node(r"\tfrac{a}{b}")[0], Node::Frac { force: Some(super::parse::Force::Text), .. }));
    assert!(matches!(&node(r"\binom{n}{k}")[0], Node::Delimited { left: Some('('), right: Some(')'), body } if matches!(body[0], Node::Frac { bar: false, .. })), "a binomial is a barless fraction in parentheses");
    assert!(matches!(&node(r"\sqrt{x}")[0], Node::Sqrt { index: None, .. }));
    assert!(matches!(&node(r"\sqrt[3]{x}")[0], Node::Sqrt { index: Some(_), .. }));
    assert!(matches!(&node(r"\sum_{i=1}^n")[0], Node::Scripts { base, .. } if matches!(**base, Node::Op { symbol: Some('\u{2211}'), wants_limits: true, .. })));
    assert!(matches!(&node(r"\int\nolimits_a")[0], Node::Scripts { base, .. } if matches!(**base, Node::Op { limits: Limits::No, .. })));
    assert!(matches!(&node(r"\sum\limits_a")[0], Node::Scripts { base, .. } if matches!(**base, Node::Op { limits: Limits::Yes, .. })));
    assert!(matches!(&node(r"\sin x")[0], Node::Op { symbol: None, wants_limits: false, .. }));
    assert!(matches!(&node(r"\lim_{x}")[0], Node::Scripts { base, .. } if matches!(**base, Node::Op { wants_limits: true, .. })));
    assert!(matches!(&node(r"\operatorname{rank}")[0], Node::Op { name, .. } if name == "rank"));
    assert!(matches!(&node(r"\left( x \right)")[0], Node::Delimited { left: Some('('), right: Some(')'), .. }));
    assert!(matches!(&node(r"\left. x \right|")[0], Node::Delimited { left: None, right: Some('|'), .. }), "a dot is no delimiter");
    assert!(matches!(&node(r"\left\langle x \right\rangle")[0], Node::Delimited { left: Some('\u{27e8}'), right: Some('\u{27e9}'), .. }));
    assert!(matches!(&node(r"\hat{x}")[0], Node::Accent { .. }));
    assert!(matches!(&node(r"\overline{x}")[0], Node::Line { over: true, .. }));
    assert!(matches!(&node(r"\underline{x}")[0], Node::Line { over: false, .. }));
    assert_eq!(node(r"\text{a b}"), [Node::Text("a b".into())], "text keeps its blanks");
    assert_eq!(node(r"\,"), [Node::Space(3.0 / 18.0)]);
    assert_eq!(node(r"\quad"), [Node::Space(1.0)]);
    assert_eq!(node(r"\!"), [Node::Space(-3.0 / 18.0)]);
}

// spec: EDIT-047
#[test]
fn environments_have_their_rows_and_columns() {
    let rows = |tex: &str| match &node(tex)[0] {
        Node::Matrix { env, rows } => (*env, rows.iter().map(Vec::len).collect::<Vec<_>>()),
        other => panic!("{other:?}"),
    };
    assert_eq!(rows(r"\begin{matrix} a & b \\ c & d \end{matrix}"), (Env::Matrix, vec![2, 2]));
    assert_eq!(rows(r"\begin{pmatrix} a \\ b \\ c \end{pmatrix}"), (Env::PMatrix, vec![1, 1, 1]));
    assert_eq!(rows(r"\begin{bmatrix} a & b \end{bmatrix}"), (Env::BMatrix, vec![2]));
    assert_eq!(rows(r"\begin{vmatrix} a & b \\ c & d \\ \end{vmatrix}"), (Env::VMatrix, vec![2, 2]), "a trailing \\\\ adds no row");
    assert_eq!(rows(r"\begin{cases} x & y \\ z & w \end{cases}"), (Env::Cases, vec![2, 2]));
    assert_eq!(rows(r"\begin{aligned} a &= b \\ c &= d \end{aligned}"), (Env::Aligned, vec![2, 2]));
}

// spec: EDIT-047
#[test]
fn every_symbol_command_the_parser_knows_has_a_glyph_in_the_font() {
    let names: Vec<&str> = GREEK.iter().chain(CAPITALS.iter()).map(|(n, _)| *n).chain(SYMBOLS.iter().map(|(n, ..)| *n)).chain(FUNCTIONS.iter().copied()).collect();
    assert!(names.len() > 150, "{}", names.len());
    for n in names {
        let tex = format!("\\{n}");
        if let Err(e) = typeset(&tex, SIZE, false) {
            panic!("{tex}: {e}");
        }
    }
    for op in ["sum", "prod", "coprod", "bigcup", "bigcap", "bigvee", "bigwedge", "bigoplus", "bigotimes", "bigsqcup", "int", "iint", "iiint", "oint"] {
        for display in [true, false] {
            if let Err(e) = typeset(&format!(r"\{op}_a^b"), SIZE, display) {
                panic!("{op}: {e}");
            }
        }
    }
    for c in ['\u{2c6}', '\u{af}', '\u{20d7}', '\u{2d9}', '\u{a8}', '\u{2dc}', '\u{2c7}', '\u{221a}'] {
        assert!(font().glyph(c).is_some(), "the font has no {c:?}");
    }
}

// spec: EDIT-047
#[test]
fn a_formula_of_every_kind_typesets() {
    for tex in [
        r"a+b-c*d/e",
        r"x^2 + y_i^{n} = \alpha\beta\gamma",
        r"\frac{a+b}{c} + \dfrac{1}{2} + \tfrac{1}{2} + \binom{n}{k}",
        r"\sqrt{x^2+1} + \sqrt[3]{y} + \sqrt{\sqrt{z}}",
        r"\sum_{i=1}^n i^2 \le \prod_k a_k \ne \int_0^1 f(x)\,dx \approx \oint_C",
        r"\lim_{x\to 0} \frac{\sin x}{x} = \max_{i} a_i \log_2 n \exp(x) \operatorname{Var}(X)",
        r"\left( \frac{a}{b} \right) \left[ x \right] \left\{ y \right\} \left| z \right| \left\| w \right\| \left\lfloor v \right\rfloor \left. u \right)",
        r"\hat{x} \bar{x} \vec{x} \dot{x} \tilde{x} \overline{xy} \underline{xy}",
        r"\text{if } x \in \mathbb{R} \mathrm{d}x \mathbf{v} \mathit{w} \mathcal{F}",
        r"a\,b\:c\;d\!e\quad f\qquad g",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix} \begin{bmatrix} 1 \end{bmatrix} \begin{vmatrix} 1 & 2 \\ 3 & 4 \end{vmatrix}",
        r"\begin{cases} x & x > 0 \\ -x & \text{else} \end{cases}",
        r"\begin{aligned} a &= b + c \\ d &= e \end{aligned}",
        r"f'(x) f''(x) A_{ij}^{kl} e^{i\pi} + 1 = 0",
        r"\infty \partial \nabla \forall \exists \to \Rightarrow \pm \times \cdot \in \subset \cup \cap",
    ] {
        for display in [true, false] {
            let l = lay(tex, display);
            assert!(l.width > 0.0 && l.ascent > 0.0 && !l.items.is_empty(), "{tex}");
        }
    }
}

// spec: EDIT-049
#[test]
fn what_cannot_be_typeset_says_why() {
    assert!(why(r"\foo{x}").contains(r"\foo"), "an unknown command is named");
    assert!(why(r"\frac{a}").contains("no argument"), "a missing argument");
    assert!(why("{x").contains("not closed"), "an open brace");
    assert!(why("x}").contains("without a {"), "a closing brace alone");
    assert!(why(r"\left( x").contains(r"\left without \right"));
    assert!(why(r"x \right)").contains(r"\right without \left"));
    assert!(why(r"\left x \right)").contains("not a delimiter"));
    assert!(why("x^").contains("no argument"));
    assert!(why("^2").contains("without a base"));
    assert!(why("x^1^2").contains("double superscript"));
    assert!(why("x_1_2").contains("double subscript"));
    assert!(why(r"\begin{matrix} a").contains("without"));
    assert!(why(r"\begin{foo} a \end{foo}").contains("foo"));
    assert!(why(r"\begin{matrix} a \end{pmatrix}").contains("closed by"));
    assert!(why("a & b").contains("environment"));
    assert!(why(r"a \\ b").contains("environment"));
    assert!(why(r"\end{matrix}").contains(r"\end without \begin"));
    assert!(why(r"\text{a \foo}").contains("not supported"));
    // spec: TEX-028 (what TeX would read as a command in running text is text only behind a backslash)
    for hostile in [r"\text{^^5cinput{x}}", r"\text{50% of}", r"\text{a # b}", r"\text{a $ b}", r"\text{a & b}", r"\text{a_b}", r"\operatorname{a^^5c}", "\\text{a\u{1}b}"] {
        assert!(parse(hostile).is_err(), "{hostile:?} is refused");
    }
    assert!(parse(r"\text{50\% of a\_b \$ \& \#}").is_ok(), "escaped, they are text");
    assert!(why(r"\sqrt[3 x").contains("not closed"));
    assert!(why(r"\limits").contains("big operator"));
    assert!(why("a \\").contains("end"));
    assert!(why("a # b").contains('#'.to_string().as_str()));
    // typeset says the same
    let e = typeset(r"\foo", SIZE, false).expect_err("an error");
    assert!(e.contains(r"\foo"), "{e}");
}

// spec: EDIT-049
#[test]
fn the_limits_are_depth_64_and_10000_characters() {
    let long = "x".repeat(MAX_CHARS);
    assert!(parse(&long).is_ok(), "exactly the limit is fine");
    let too_long = "x".repeat(MAX_CHARS + 1);
    assert!(why(&too_long).contains("10000"), "one over: {}", why(&too_long));
    let nest = |n: usize| format!("{}x{}", "{".repeat(n), "}".repeat(n));
    assert!(parse(&nest(MAX_DEPTH - 2)).is_ok(), "deep, but within the limit");
    assert!(why(&nest(MAX_DEPTH + 2)).contains("64"), "too deep");
    assert!(why(&format!("{}x{}", r"\frac{1}{".repeat(80), "}".repeat(80))).contains("64"), "fractions nest too");
    assert!(why(&format!("x{}", "^{y".repeat(200) + &"}".repeat(200))).contains("64"), "scripts nest too");
    // a long formula within the limits still typesets, and quickly
    let started = std::time::Instant::now();
    assert!(layout(&parse(&long).unwrap(), 14.0, false).is_ok());
    let long_sum = "a+b=c ".repeat(1600);
    assert!(layout(&parse(&long_sum).unwrap(), 14.0, true).is_ok());
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
}

/// A small deterministic generator, so the garbled inputs are the same in every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

// spec: EDIT-049
#[test]
fn garbled_formulas_never_panic_or_hang() {
    let pieces = [
        r"\frac",
        r"\sqrt",
        "[",
        "]",
        "{",
        "}",
        "^",
        "_",
        r"\left",
        r"\right",
        "(",
        ")",
        "|",
        ".",
        "&",
        r"\\",
        r"\begin{matrix}",
        r"\end{matrix}",
        r"\begin{cases}",
        r"\end{cases}",
        r"\begin{aligned}",
        "x",
        "1",
        "'",
        r"\sum",
        r"\int",
        r"\limits",
        r"\nolimits",
        r"\text",
        r"\text{a}",
        r"\hat",
        r"\overline",
        r"\mathbb",
        r"\operatorname",
        r"\binom",
        r"\alpha",
        r"\foo",
        " ",
        "$",
        "%",
        "é",
        "日",
        "😀",
        "\u{0}",
        "\n",
        r"\,",
        r"\quad",
        "+",
        "=",
        "-",
        r"\left(",
        r"\right)",
        r"\begin",
        r"\end",
        r"\lim",
        r"\\\\",
        "~",
        "#",
        "\"",
    ];
    let sound = [r"\frac{a}{b}", r"x^2", r"\sqrt[3]{y}", r"\left( z \right)", r"\sum_{i=1}^n", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", r"\hat{x} + \alpha", r"\text{if } x", r"\int_0^1 f\,dx"];
    let mut rng = Lcg(0x5eed);
    let mut ok = 0;
    let started = std::time::Instant::now();
    for round in 0..4000 {
        let n = 1 + rng.below(40);
        let mut s = String::new();
        if round % 2 == 0 {
            // sound pieces with a character or two dropped, doubled or changed
            for _ in 0..1 + rng.below(4) {
                s.push_str(sound[rng.below(sound.len())]);
            }
            let mut chars: Vec<char> = s.chars().collect();
            for _ in 0..rng.below(3) {
                let at = rng.below(chars.len());
                match rng.below(3) {
                    0 => {
                        chars.remove(at);
                    }
                    1 => chars.insert(at, chars[at]),
                    _ => chars[at] = pieces[rng.below(pieces.len())].chars().next().unwrap_or('x'),
                }
            }
            s = chars.into_iter().collect();
        } else {
            for _ in 0..n {
                s.push_str(pieces[rng.below(pieces.len())]);
            }
        }
        match parse(&s) {
            Ok(tree) => {
                ok += 1;
                // whatever parses also lays out, in both styles, without panicking
                let _ = layout(&tree, 12.0, round % 2 == 0);
            }
            Err(e) => assert!(!e.0.is_empty(), "an error has a reason: {s:?}"),
        }
    }
    assert!(ok > 800, "the garbling is not all garbage: {ok} of 4000 parsed");
    // random characters, too
    for _ in 0..1500 {
        let s: String = (0..rng.below(60)).filter_map(|_| char::from_u32(rng.next() as u32 % 0x3000)).collect();
        let _ = parse(&s).map(|t| layout(&t, 12.0, false));
    }
    assert!(started.elapsed().as_secs() < 60, "{:?}", started.elapsed());
}

// spec: EDIT-047
#[test]
fn a_superscript_is_raised_and_smaller_and_a_subscript_lowered() {
    let l = lay("x^2", false);
    let (bx, by, bs) = one(&l, '\u{1D465}');
    let (sx, sy, ss) = one(&l, '2');
    assert_eq!(by, 0.0, "the base is on the baseline");
    assert!(sy > by + 0.3 * SIZE, "raised: {sy}");
    assert!(ss < bs * 0.8 && ss > bs * 0.6, "script size: {ss} of {bs}");
    assert!(sx > bx, "after the base");
    let l = lay("x_2", false);
    let (_, sy, _) = one(&l, '2');
    assert!(sy < -0.1 * SIZE, "lowered: {sy}");
    let l = lay("x^2_3", false);
    let (up, down) = (one(&l, '2'), one(&l, '3'));
    assert!(up.1 > 0.0 && down.1 < 0.0 && (up.0 - down.0).abs() < 0.2 * SIZE, "both at the same x, one above and one below: {up:?} {down:?}");
    // a script of a script is smaller still
    let l = lay("x^{y^z}", false);
    assert!(one(&l, '\u{1D467}').2 < one(&l, '\u{1D466}').2);
    // a raised script makes the formula taller, a lowered one deeper
    assert!(lay("x^2", false).ascent > lay("x", false).ascent);
    assert!(lay("x_2", false).descent > lay("x", false).descent);
}

// spec: EDIT-047
#[test]
fn a_fraction_has_the_numerator_above_the_bar_above_the_denominator() {
    let l = lay(r"\frac{a}{b}", true);
    let r = rules(&l);
    assert_eq!(r.len(), 1, "one bar");
    let (rx, ry, rw, rh) = r[0];
    let (a, b) = (one(&l, '\u{1D44E}'), one(&l, '\u{1D44F}'));
    assert!(a.1 > ry + rh && b.1 + 0.5 * SIZE < ry, "a {a:?} above the bar at {ry}, b {b:?} below");
    assert!(rw >= 0.5 * SIZE && rx >= 0.0 && rx + rw <= l.width, "the bar spans the fraction");
    // both are centred over the bar
    let (ca, cb) = (a.0 + 0.5 * font().advance(gid('\u{1D44E}')) / font().upem() * a.2, b.0 + 0.5 * font().advance(gid('\u{1D44F}')) / font().upem() * b.2);
    assert!((ca - cb).abs() < 0.1 * SIZE, "centred: {ca} {cb}");
    // the bar is on the math axis
    let axis = font().constant(skrifa::raw::tables::math::MathConstant::AxisHeight) * SIZE / font().upem();
    assert!((ry + rh / 2.0 - axis).abs() < 0.5, "axis {axis}, bar {ry}");
    // in text style the parts are smaller than in display style
    assert!(one(&lay(r"\frac{a}{b}", false), '\u{1D44E}').2 < a.2);
    // a binomial has no bar
    let b = lay(r"\binom{n}{k}", true);
    assert!(rules(&b).is_empty() && one(&b, '\u{1D45B}').1 > one(&b, '\u{1D458}').1);
    // a fraction in a fraction is still smaller
    let nested = lay(r"\frac{\frac{a}{b}}{c}", true);
    assert_eq!(rules(&nested).len(), 2);
    assert!(one(&nested, '\u{1D44E}').2 < one(&nested, '\u{1D450}').2 + 0.01);
}

// spec: EDIT-047
#[test]
fn big_operators_have_limits_below_and_above_in_display_and_beside_inline() {
    let sigma = '\u{2211}';
    // the symbol of a big operator in display style is a larger glyph of the font: the tallest item
    let symbol = |l: &Layout| {
        l.items
            .iter()
            .filter_map(|i| match i {
                Item::Glyph { gid, x, size, .. } => ink(i).map(|b| (b.3 - b.2, *gid, *x, *size)),
                _ => None,
            })
            .fold((0.0, gid(sigma), 0.0, SIZE), |best, c| if c.0 > best.0 { c } else { best })
    };
    let display = lay(r"\sum_{i}^{n} x", true);
    let (tall, big, sx, ss) = symbol(&display);
    let (ix, iy, _) = one(&display, '\u{1D456}');
    let (nx, ny, _) = one(&display, '\u{1D45B}');
    let wide = font().advance(big) / font().upem() * ss;
    assert_ne!(big, gid(sigma), "the larger variant of the sum in display style");
    assert!(tall > 1.2 * SIZE, "{tall}");
    assert!(iy < 0.0 && ny > SIZE * 0.5, "below and above the symbol: {iy} {ny}");
    assert!(ix + 0.4 * SIZE > sx && ix < sx + wide && nx > sx - 0.1 * SIZE && nx < sx + wide, "centred over the symbol: sum at {sx}, i at {ix}, n at {nx}");
    // the symbol is the bigger variant in display style
    let inline = lay(r"\sum_{i}^{n} x", false);
    let (ix2, iy2, _) = one(&inline, '\u{1D456}');
    let (nx2, ny2, _) = one(&inline, '\u{1D45B}');
    let (sx2, _, ss2) = one(&inline, sigma);
    let wide2 = font().advance(gid(sigma)) / font().upem() * ss2;
    assert_eq!(symbol(&inline).1, gid(sigma), "the plain sum inline");
    assert!(ix2 >= sx2 + wide2 - 1.0 && nx2 >= sx2 + wide2 - 1.0, "beside the symbol inline: {ix2} {nx2} {sx2}");
    assert!(iy2 < 0.0 && ny2 > 0.0 && (ny2 - iy2) < (ny - iy), "scripts close in");
    assert!(display.ascent + display.descent > inline.ascent + inline.descent, "display is taller");
    // \limits and \nolimits override
    let forced = lay(r"\sum\nolimits_{i}^{n}", true);
    let (_, _, fx, _) = symbol(&forced);
    assert!(one(&forced, '\u{1D456}').0 > fx + 0.5 * SIZE, "beside although in display style");
    let forced = lay(r"\int\limits_{i}^{n}", true);
    assert!(one(&forced, '\u{1D456}').1 < 0.0 && one(&forced, '\u{1D45B}').1 > SIZE, "an integral with limits above and below");
    // an integral has its scripts beside it in display style
    let int = lay(r"\int_a^b", true);
    let (_, _, int_x, _) = symbol(&int);
    assert!(one(&int, '\u{1D44F}').0 > int_x && one(&int, '\u{1D44E}').1 < 0.0 && one(&int, '\u{1D44F}').1 > 0.0, "beside the integral, one below and one above");
    // `lim` has them below in display, `sin` never
    let lim = lay(r"\lim_{x}", true);
    assert!(one(&lim, '\u{1D465}').1 < -0.2 * SIZE && one(&lim, '\u{1D465}').0 < lim.width * 0.7);
    let sin = lay(r"\sin_{x}", true);
    assert!(one(&sin, '\u{1D465}').0 > one(&sin, 'n').0, "beside");
}

// spec: EDIT-047
#[test]
fn delimiters_grow_with_what_they_hold() {
    let small = lay(r"\left( x \right)", true);
    let tall = lay(r"\left( \frac{a}{\frac{b}{c}} \right)", true);
    let height = |l: &Layout| l.ascent + l.descent;
    assert!(height(&tall) > height(&small) * 1.5, "{} {}", height(&tall), height(&small));
    // the parenthesis itself is a bigger glyph
    let paren = |l: &Layout| {
        l.items.iter().find_map(|i| match i {
            Item::Glyph { gid: g, .. } if ink(i).is_some_and(|b| b.1 - b.0 < 0.7 * SIZE && b.3 - b.2 > 0.8 * SIZE) => Some((*g, ink(i).unwrap())),
            _ => None,
        })
    };
    let (g_small, b_small) = paren(&small).expect("a paren");
    let (g_tall, b_tall) = paren(&tall).expect("a paren");
    assert_ne!(g_small, g_tall, "another variant");
    assert!(b_tall.3 - b_tall.2 > (b_small.3 - b_small.2) * 1.4);
    // and it covers its content
    assert!(b_tall.3 >= tall.ascent - 0.3 * SIZE && b_tall.2 <= -tall.descent + 0.3 * SIZE, "{b_tall:?} {} {}", tall.ascent, tall.descent);
    // very tall content is an assembly of several parts of the font
    let huge = lay(r"\left\{ \begin{matrix} 1 \\ 2 \\ 3 \\ 4 \\ 5 \\ 6 \\ 7 \\ 8 \end{matrix} \right.", true);
    let parts = huge.items.iter().filter(|i| matches!(i, Item::Glyph { gid: g, .. } if ink(i).is_some_and(|b| b.1 - b.0 < 0.6 * SIZE && b.3 - b.2 > 0.5 * SIZE && *g != gid('{')))).count();
    assert!(parts >= 3, "a brace made of parts: {parts}");
    // delimiters are centred on the axis
    let axis = font().constant(skrifa::raw::tables::math::MathConstant::AxisHeight) * SIZE / font().upem();
    assert!(((b_tall.2 + b_tall.3) / 2.0 - axis).abs() < 0.15 * SIZE);
    // a `\left.` has no left delimiter
    assert!(lay(r"\left. x \right)", false).width < lay(r"\left( x \right)", false).width);
}

// spec: EDIT-047
#[test]
fn matrices_are_aligned_in_columns_and_rows() {
    let m = lay(r"\begin{matrix} 1 & 22 \\ 333 & 4 \end{matrix}", false);
    let centre = |c: char| {
        let (x, y, s) = at(&m, c)[0];
        (x + font().advance(gid(c)) / font().upem() * s / 2.0, y)
    };
    let (c1, c3) = (centre('1'), centre('3'));
    assert!(at(&m, '3').len() == 3);
    let third = at(&m, '3');
    let (x0, x2) = (third[0].0, third[2].0 + font().advance(gid('3')) / font().upem() * third[2].2);
    assert!((c1.0 - (x0 + x2) / 2.0).abs() < 0.6, "the first column is centred: {} {}", c1.0, (x0 + x2) / 2.0);
    assert!(c1.1 > c3.1 + 0.8 * SIZE, "the first row is above the second: {c1:?} {c3:?}");
    let (c4, c2) = (centre('4'), at(&m, '2'));
    assert!(c4.0 > centre('1').0 + 1.5 * SIZE && c2[0].0 > x2, "the second column is right of the first: {c4:?}");
    assert!((c2[0].1 - c1.1).abs() < 0.01, "the cells of a row share a baseline");
    // the matrix is centred on the axis
    let axis = font().constant(skrifa::raw::tables::math::MathConstant::AxisHeight) * SIZE / font().upem();
    assert!(((m.ascent - m.descent) / 2.0 - axis).abs() < 0.5, "centred on the axis: {} {}", m.ascent, m.descent);
    // pmatrix: with parentheses around, wider and as tall
    let p = lay(r"\begin{pmatrix} 1 & 22 \\ 333 & 4 \end{pmatrix}", false);
    assert!(p.width > m.width + 0.4 * SIZE && p.ascent >= m.ascent);
    // cases: the columns are left-aligned
    let c = lay(r"\begin{cases} a & b \\ ccc & d \end{cases}", false);
    let a = one(&c, '\u{1D44E}');
    let first_c = at(&c, '\u{1D450}');
    assert!((a.0 - first_c[0].0).abs() < 0.2, "left-aligned: {} {}", a.0, first_c[0].0);
    // aligned: the first column right-aligned, the second left-aligned
    let al = lay(r"\begin{aligned} a &= b \\ ccc &= d \end{aligned}", false);
    let eq = at(&al, '=');
    assert!((eq[0].0 - eq[1].0).abs() < 0.2, "the = signs are under each other: {eq:?}");
    let a = one(&al, '\u{1D44E}');
    let c0 = at(&al, '\u{1D450}');
    assert!(a.0 > c0[0].0 + 0.5 * SIZE, "a is right-aligned against the ccc: {a:?} {c0:?}");
    // a matrix with a missing cell is still a matrix
    assert!(typeset(r"\begin{matrix} a \\ b & c \end{matrix}", SIZE, false).is_ok());
}

// spec: EDIT-047
#[test]
fn a_root_has_its_rule_over_the_body_and_the_index_small_and_raised_before_it() {
    let r = lay(r"\sqrt{x}", false);
    let rule = rules(&r);
    assert_eq!(rule.len(), 1);
    let (rx, ry, rw, _) = rule[0];
    let (bx, by, _) = one(&r, '\u{1D465}');
    assert!(ry > font().bounds(gid('x')).y1 * SIZE / font().upem() && rx <= bx + 0.5 && rx + rw >= bx + 0.4 * SIZE, "the rule is above the body and spans it: rule {rule:?}, x at {bx}");
    assert_eq!(by, 0.0);
    // the radical is a glyph of its own before the body, reaching below the baseline
    let radical = r.items.iter().find_map(|i| match i {
        Item::Glyph { gid: g, .. } if *g != gid('\u{1D465}') => ink(i),
        _ => None,
    });
    let radical = radical.expect("a radical glyph");
    assert!(radical.1 <= bx + 1.5 && radical.2 < 0.0, "{radical:?}");
    // taller bodies give a taller radical
    let tall = lay(r"\sqrt{\frac{a}{b}}", true);
    let radical_tall = tall.items.iter().filter_map(ink).map(|b| b.3 - b.2).fold(0.0, f32::max);
    assert!(radical_tall > (radical.3 - radical.2) * 1.3, "{radical_tall}");
    // an index
    let n = lay(r"\sqrt[3]{x}", false);
    let (ix, iy, is) = one(&n, '3');
    let (bx3, _, bs) = one(&n, '\u{1D465}');
    assert!(is < bs * 0.7, "the index is in scriptscript size: {is} {bs}");
    assert!(ix < bx3 - 0.2 * SIZE && iy > 0.2 * SIZE, "before the root and raised: {ix} {iy} {bx3}");
    assert!(n.width > r.width, "the index takes room");
}

// spec: EDIT-047
#[test]
fn atoms_get_the_spaces_of_their_classes() {
    // a relation is spaced more than a binary operator, which is spaced more than nothing
    let gap = |tex: &str, left: char, right: char| {
        let l = lay(tex, false);
        let (lx, _, ls) = one(&l, left);
        let (rx, _, _) = one(&l, right);
        rx - (lx + font().advance(gid(left)) / font().upem() * ls)
    };
    let ord = gap("ab", '\u{1D44E}', '\u{1D44F}');
    let bin = gap("a+b", '\u{1D44E}', '+');
    let rel = gap("a=b", '\u{1D44E}', '=');
    assert!(bin > ord + 0.1 * SIZE && rel > bin + 0.02 * SIZE, "{ord} {bin} {rel}");
    // a binary operator at the start has nothing to operate on: no space
    let lone = lay("-a", false);
    let (mx, _, ms) = one(&lone, '\u{2212}');
    let (ax, _, _) = one(&lone, '\u{1D44E}');
    let (mx2, _, _) = one(&lay("b-a", false), '\u{2212}');
    assert!(ax - (mx + font().advance(gid('\u{2212}')) / font().upem() * ms) < 0.1 * SIZE && mx2 > 0.0, "no space after a leading minus");
    // no spaces in script style
    let script = lay("x^{a=b}", false);
    let (ex, _, _) = one(&script, '=');
    let (ax, _, as_) = one(&script, '\u{1D44E}');
    assert!(ex - (ax + font().advance(gid('\u{1D44E}')) / font().upem() * as_) < 0.1 * SIZE, "script style has tight relations");
    // explicit spaces
    assert!(lay(r"a\quad b", false).width > lay("ab", false).width + 0.9 * SIZE);
    assert!(lay(r"a\,b", false).width > lay("ab", false).width);
    assert!(lay(r"a\!b", false).width < lay("ab", false).width);
    // text is upright
    assert!(at(&lay(r"\text{a}", false), 'a').len() == 1 && at(&lay("a", false), 'a').is_empty());
}

// spec: EDIT-047
#[test]
fn accents_and_lines_sit_above_and_below_their_base() {
    let base = lay("x", false);
    let hat = lay(r"\hat{x}", false);
    let (_, hy, _) = one(&hat, '\u{2c6}');
    assert!(hat.ascent > base.ascent && hy >= 0.0, "the hat is on top: {hy}");
    let tall = lay(r"\hat{A}", false);
    assert!(one(&tall, '\u{2c6}').1 > hy + 0.1 * SIZE, "over a capital the hat is higher");
    let o = lay(r"\overline{x}", false);
    let r = rules(&o);
    assert_eq!(r.len(), 1);
    assert!(r[0].1 > base.ascent && r[0].2 >= base.width - 0.1, "{r:?} {}", base.ascent);
    let u = lay(r"\underline{x}", false);
    assert!(rules(&u)[0].1 < -0.05 * SIZE && u.descent > base.descent);
}

// spec: EDIT-047
#[test]
fn display_style_is_bigger_than_text_style_and_the_size_scales_the_formula() {
    let d = lay(r"\frac{a+b}{c}", true);
    let t = lay(r"\frac{a+b}{c}", false);
    assert!(d.ascent + d.descent > t.ascent + t.descent);
    let small = typeset(r"x^2 + \frac{a}{b}", 10.0, false).unwrap();
    let big = typeset(r"x^2 + \frac{a}{b}", 20.0, false).unwrap();
    let ratio = big.layout.width / small.layout.width;
    assert!((ratio - 2.0).abs() < 0.01, "{ratio}");
    assert!((big.layout.ascent / small.layout.ascent - 2.0).abs() < 0.01);
}

// spec: EDIT-048
#[test]
fn layouts_are_cached_by_tex_size_and_style() {
    let a = typeset(r"\frac{cache}{key} + 17", 13.37, false).unwrap();
    let b = typeset(r"\frac{cache}{key} + 17", 13.37, false).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "the same TeX, size and style is laid out once");
    let spaced = typeset(r"  \frac{cache}{key} + 17  ", 13.37, false).unwrap();
    assert!(Arc::ptr_eq(&a, &spaced), "blanks around the TeX do not matter");
    assert!(!Arc::ptr_eq(&a, &typeset(r"\frac{cache}{key} + 17", 13.37, true).unwrap()), "another style");
    assert!(!Arc::ptr_eq(&a, &typeset(r"\frac{cache}{key} + 17", 14.0, false).unwrap()), "another size");
    // an error is cached, too
    let e1 = typeset(r"\nosuchcommand{cache}", 13.37, false).err().unwrap();
    let e2 = typeset(r"\nosuchcommand{cache}", 13.37, false).err().unwrap();
    assert_eq!(e1, e2);
}

// spec: EDIT-048
#[test]
fn the_mask_has_ink_where_the_formula_is_and_none_far_away() {
    let l = lay(r"\frac{1}{2}", true);
    let m = rasterise(&l, 2.0);
    assert_eq!(m.alpha.len(), m.width * m.height);
    assert!(m.width as f32 >= l.width * 2.0 && m.height as f32 >= l.height() * 2.0);
    let inked = m.alpha.iter().filter(|a| **a > 128).count();
    assert!(inked > 100 && inked < m.alpha.len() / 2, "{inked} of {}", m.alpha.len());
    // the bar of the fraction is a solid row
    let bar = rules(&l)[0];
    let row = (m.baseline - (bar.1 + bar.3 / 2.0) * 2.0).round() as usize;
    let solid = (0..m.width).filter(|x| m.alpha[row * m.width + x] > 200).count();
    assert!(solid as f32 > bar.2 * 2.0 * 0.9, "the bar is drawn: {solid} of {}", bar.2 * 2.0);
    // the margin is empty
    assert!((0..m.height).all(|y| m.alpha[y * m.width] < 20 && m.alpha[y * m.width + m.width - 1] < 20));
    // a finer screen gives a bigger mask of the same shape
    let m1 = rasterise(&l, 1.0);
    assert!(m.width > m1.width && m.height > m1.height);
    assert_eq!((m.scale, m1.scale), (2.0, 1.0));
    // a huge formula is made at a smaller scale, not with a huge picture
    let huge = Layout { width: 30_000.0, ascent: 3000.0, descent: 3000.0, items: vec![Item::Rule { x: 0.0, y: -10.0, w: 30_000.0, h: 20.0 }] };
    let big = rasterise(&huge, 2.0);
    assert!(big.scale < 0.2 && big.width * big.height <= 6_500_000, "{} {}x{}", big.scale, big.width, big.height);
}
