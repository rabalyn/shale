//! The TeX subset of EDIT-047 as a tree (EDIT-049): what is supported, what is not and why.
//! Letters are already in their math alphabet (italic, bold, double-struck, script) when parsed, so
//! the layout only looks up glyphs.

/// The longest formula, in characters (EDIT-049).
pub const MAX_CHARS: usize = 10_000;
/// The deepest nesting of groups, fractions, scripts … (EDIT-049).
pub const MAX_DEPTH: usize = 64;

/// Why a formula cannot be typeset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(reason: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError(reason.into()))
}

/// The kind of an atom: how much space it gets next to its neighbours (TeXbook chapter 18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Ord,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

/// Whether a big operator puts its scripts above and below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limits {
    /// above and below in display style, beside otherwise
    Default,
    /// `\limits`
    Yes,
    /// `\nolimits`
    No,
}

/// The shape of a matrix-like environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Env {
    Matrix,
    PMatrix,
    BMatrix,
    VMatrix,
    Cases,
    Aligned,
}

impl Env {
    fn named(name: &str) -> Option<Env> {
        Some(match name {
            "matrix" => Env::Matrix,
            "pmatrix" => Env::PMatrix,
            "bmatrix" => Env::BMatrix,
            "vmatrix" => Env::VMatrix,
            "cases" => Env::Cases,
            "aligned" => Env::Aligned,
            _ => return None,
        })
    }
}

/// The size a fraction is forced to (`\dfrac`, `\tfrac`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    Display,
    Text,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// one symbol (a letter in its alphabet, a number's digit, an operator …)
    Char(char, Class),
    /// upright running text (`\text`)
    Text(String),
    /// a braced group or a whole row
    Group(Vec<Node>),
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    Frac {
        num: Box<Node>,
        den: Box<Node>,
        bar: bool,
        force: Option<Force>,
    },
    Sqrt {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// a big operator (`\sum`: the symbol) or a named one (`\sin`, `\lim`)
    Op {
        symbol: Option<char>,
        name: String,
        limits: Limits,
        wants_limits: bool,
    },
    Delimited {
        left: Option<char>,
        right: Option<char>,
        body: Vec<Node>,
    },
    Accent {
        mark: char,
        body: Box<Node>,
    },
    Line {
        over: bool,
        body: Box<Node>,
    },
    /// a space in em, negative for `\!`
    Space(f32),
    Matrix {
        env: Env,
        rows: Vec<Vec<Node>>,
    },
}

/// Parse a formula. Never panics and never loops: every step eats input and the nesting is bounded.
pub fn parse(src: &str) -> Result<Node, ParseError> {
    if src.chars().count() > MAX_CHARS {
        return err(format!("the formula is longer than {MAX_CHARS} characters"));
    }
    let mut p = Parser { s: src.chars().collect(), i: 0, depth: 0, font: Font::Italic };
    let row = p.row(End::Eof)?;
    Ok(Node::Group(row))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Font {
    Italic,
    Roman,
    Bold,
    Blackboard,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Eof,
    Brace,
    Right,
    /// inside an environment: ends at `\end`, `&` or `\\`
    Cell,
}

struct Parser {
    s: Vec<char>,
    i: usize,
    depth: usize,
    font: Font,
}

/// A token ahead.
enum Tok {
    Char(char),
    Command(String),
}

impl Parser {
    fn peek_char(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek_char().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    /// The next token without eating it: its length is returned with it.
    fn look(&self) -> Option<(Tok, usize)> {
        let c = self.peek_char()?;
        if c != '\\' {
            return Some((Tok::Char(c), 1));
        }
        let mut j = self.i + 1;
        match self.s.get(j) {
            None => Some((Tok::Command(String::new()), 1)),
            Some(&n) if n.is_ascii_alphabetic() => {
                let mut name = String::new();
                while let Some(&l) = self.s.get(j).filter(|l| l.is_ascii_alphabetic()) {
                    name.push(l);
                    j += 1;
                }
                Some((Tok::Command(name), j - self.i))
            }
            Some(&n) => Some((Tok::Command(n.to_string()), 2)),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return err(format!("nested deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }

    /// Nodes up to the end that `end` names (a `}` is eaten, the others are left to the caller).
    fn row(&mut self, end: End) -> Result<Vec<Node>, ParseError> {
        self.enter()?;
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            let Some((tok, _)) = self.look() else {
                return match end {
                    End::Eof => break,
                    End::Brace => err("a { is not closed"),
                    End::Right => err("\\left without \\right"),
                    End::Cell => err("\\begin without \\end"),
                };
            };
            match &tok {
                Tok::Char('}') => {
                    if end == End::Brace {
                        self.i += 1;
                        break;
                    }
                    return err("a } without a {");
                }
                Tok::Char('&') => {
                    if end == End::Cell {
                        break;
                    }
                    return err("& outside of an environment");
                }
                Tok::Command(c) if c == "\\" => {
                    if end == End::Cell {
                        break;
                    }
                    return err("\\\\ outside of an environment");
                }
                Tok::Command(c) if c == "right" => {
                    if end == End::Right {
                        break;
                    }
                    return err("\\right without \\left");
                }
                Tok::Command(c) if c == "end" => {
                    if end == End::Cell {
                        break;
                    }
                    return err("\\end without \\begin");
                }
                Tok::Char('^' | '_') => return err("a script without a base"),
                _ => {}
            }
            let atom = self.atom()?;
            let atom = self.scripts(atom)?;
            out.push(atom);
        }
        self.depth -= 1;
        Ok(out)
    }

    /// The scripts that follow an atom (`^`, `_`, primes), in either order. `\limits` after a big
    /// operator is read first, so that `\int\limits_a^b` works.
    fn scripts(&mut self, base: Node) -> Result<Node, ParseError> {
        let mut base = base;
        let (mut sub, mut sup): (Option<Node>, Option<Node>) = (None, None);
        let mut primes = 0;
        loop {
            self.skip_ws();
            if let (Some((Tok::Command(c), len)), Node::Op { limits, .. }) = (self.look(), &mut base) {
                if c == "limits" || c == "nolimits" {
                    *limits = if c == "limits" { Limits::Yes } else { Limits::No };
                    self.i += len;
                    continue;
                }
            }
            match self.peek_char() {
                Some('^') => {
                    self.i += 1;
                    if sup.is_some() {
                        return err("a double superscript");
                    }
                    sup = Some(self.argument("^")?);
                }
                Some('_') => {
                    self.i += 1;
                    if sub.is_some() {
                        return err("a double subscript");
                    }
                    sub = Some(self.argument("_")?);
                }
                Some('\'') => {
                    self.i += 1;
                    if sup.is_some() && primes == 0 {
                        return err("a double superscript");
                    }
                    primes += 1;
                    sup = Some(Node::Group(vec![Node::Char(['′', '″', '‴', '⁗'][(primes - 1usize).min(3)], Class::Ord)]));
                }
                _ => break,
            }
        }
        if sub.is_none() && sup.is_none() {
            return Ok(base);
        }
        Ok(Node::Scripts { base: Box::new(base), sub: sub.map(Box::new), sup: sup.map(Box::new) })
    }

    /// The argument of a command or a script: a group, or the next single token.
    fn argument(&mut self, of: &str) -> Result<Node, ParseError> {
        self.skip_ws();
        match self.look() {
            None | Some((Tok::Char('}' | '&' | '^' | '_'), _)) => err(format!("{of} has no argument")),
            Some((Tok::Command(c), _)) if matches!(c.as_str(), "right" | "end" | "\\") => err(format!("{of} has no argument")),
            Some((Tok::Char('{'), _)) => {
                self.i += 1;
                Ok(Node::Group(self.row(End::Brace)?))
            }
            Some(_) => {
                self.enter()?;
                let n = self.atom();
                self.depth -= 1;
                n
            }
        }
    }

    /// The text of `{…}` as it is written, for `\text` and names.
    fn raw_group(&mut self, of: &str) -> Result<String, ParseError> {
        self.skip_ws();
        if self.peek_char() != Some('{') {
            return err(format!("{of} needs a {{…}} argument"));
        }
        self.i += 1;
        let mut out = String::new();
        let mut depth = 0usize;
        loop {
            let Some(c) = self.peek_char() else { return err(format!("the {{ of {of} is not closed")) };
            self.i += 1;
            match c {
                '{' => {
                    depth += 1;
                    out.push(c);
                }
                '}' if depth == 0 => return Ok(out),
                '}' => {
                    depth -= 1;
                    out.push(c);
                }
                '\\' => match self.peek_char() {
                    Some(n @ ('{' | '}' | '$' | '%' | '&' | '#' | '_')) => {
                        self.i += 1;
                        out.push(n);
                    }
                    Some(' ') => {
                        self.i += 1;
                        out.push(' ');
                    }
                    _ => return err(format!("a command inside {of} is not supported")),
                },
                // TeX reads these as commands or errors in running text (`%` comments out the rest of the
                // line, `^^5c` is a backslash): they are text only behind a backslash (TEX-029)
                '%' | '#' | '&' | '^' | '_' | '$' | '~' => return err(format!("{c} inside {of} must be written \\{c}")),
                c if c.is_control() && !c.is_whitespace() => return err(format!("a control character inside {of}")),
                c => out.push(c),
            }
        }
    }

    fn with_font(&mut self, font: Font, of: &str) -> Result<Node, ParseError> {
        let old = std::mem::replace(&mut self.font, font);
        let n = self.argument(of);
        self.font = old;
        n
    }

    /// One atom: a symbol, a group or a command with its arguments.
    fn atom(&mut self) -> Result<Node, ParseError> {
        let Some((tok, len)) = self.look() else { return err("the formula ends early") };
        self.i += len;
        match tok {
            Tok::Char(c) => self.plain(c),
            Tok::Command(name) => self.command(&name),
        }
    }

    fn plain(&mut self, c: char) -> Result<Node, ParseError> {
        Ok(match c {
            '{' => Node::Group(self.row(End::Brace)?),
            'a'..='z' | 'A'..='Z' | '0'..='9' => Node::Char(styled(c, self.font), Class::Ord),
            '+' => Node::Char('+', Class::Bin),
            '-' => Node::Char('\u{2212}', Class::Bin),
            '*' => Node::Char('\u{2217}', Class::Bin),
            '=' | '<' | '>' | ':' => Node::Char(c, Class::Rel),
            '(' | '[' => Node::Char(c, Class::Open),
            ')' | ']' => Node::Char(c, Class::Close),
            ',' | ';' => Node::Char(c, Class::Punct),
            '|' | '/' | '!' | '.' | '?' | '@' => Node::Char(c, Class::Ord),
            '~' => Node::Space(0.25),
            '\u{80}'.. => Node::Char(c, Class::Ord),
            _ => return err(format!("the character {c} is not supported")),
        })
    }

    fn command(&mut self, name: &str) -> Result<Node, ParseError> {
        if let Some((ch, class)) = symbol(name) {
            let greek = ('\u{3b1}'..='\u{3c9}').contains(&ch) || "ϑϰϕϱϖϵ".contains(ch);
            return Ok(Node::Char(if greek { styled(ch, self.font) } else { ch }, class));
        }
        if let Some((symbol, limits)) = big_operator(name) {
            return Ok(Node::Op { symbol: Some(symbol), name: String::new(), limits: Limits::Default, wants_limits: limits });
        }
        if FUNCTIONS.contains(&name) {
            return Ok(Node::Op { symbol: None, name: name.to_string(), limits: Limits::Default, wants_limits: LIMIT_FUNCTIONS.contains(&name) });
        }
        if let Some(em) = space(name) {
            return Ok(Node::Space(em));
        }
        Ok(match name {
            "frac" | "dfrac" | "tfrac" | "binom" => {
                let num = self.argument(name)?;
                let den = self.argument(name)?;
                let force = match name {
                    "dfrac" => Some(Force::Display),
                    "tfrac" => Some(Force::Text),
                    _ => None,
                };
                let frac = Node::Frac { num: Box::new(num), den: Box::new(den), bar: name != "binom", force };
                if name == "binom" {
                    Node::Delimited { left: Some('('), right: Some(')'), body: vec![frac] }
                } else {
                    frac
                }
            }
            "sqrt" => {
                self.skip_ws();
                let index = if self.peek_char() == Some('[') {
                    self.i += 1;
                    let start = self.i;
                    let mut depth = 0usize;
                    while let Some(c) = self.peek_char() {
                        match c {
                            '{' => depth += 1,
                            '}' => depth = depth.saturating_sub(1),
                            ']' if depth == 0 => break,
                            _ => {}
                        }
                        self.i += 1;
                    }
                    if self.peek_char() != Some(']') {
                        return err("the [ of \\sqrt is not closed");
                    }
                    let inner: String = self.s[start..self.i].iter().collect();
                    self.i += 1;
                    let mut sub = Parser { s: inner.chars().collect(), i: 0, depth: self.depth, font: self.font };
                    Some(Box::new(Node::Group(sub.row(End::Eof)?)))
                } else {
                    None
                };
                Node::Sqrt { index, body: Box::new(self.argument("\\sqrt")?) }
            }
            "left" => return self.delimited(),
            "begin" => return self.environment(),
            "operatorname" => {
                let name = self.raw_group("\\operatorname")?;
                if name.is_empty() {
                    return err("\\operatorname has no name");
                }
                let wants_limits = LIMIT_FUNCTIONS.contains(&name.as_str());
                Node::Op { symbol: None, name, limits: Limits::Default, wants_limits }
            }
            "text" | "textrm" | "mbox" => Node::Text(self.raw_group(&format!("\\{name}"))?),
            "mathrm" => self.with_font(Font::Roman, "\\mathrm")?,
            "mathbf" => self.with_font(Font::Bold, "\\mathbf")?,
            "mathit" => self.with_font(Font::Italic, "\\mathit")?,
            "mathbb" => self.with_font(Font::Blackboard, "\\mathbb")?,
            "mathcal" => self.with_font(Font::Script, "\\mathcal")?,
            "hat" | "bar" | "vec" | "dot" | "ddot" | "tilde" | "check" => {
                let mark = match name {
                    "hat" => '\u{2c6}',
                    "bar" => '\u{af}',
                    "vec" => '\u{20d7}',
                    "dot" => '\u{2d9}',
                    "ddot" => '\u{a8}',
                    "tilde" => '\u{2dc}',
                    _ => '\u{2c7}',
                };
                Node::Accent { mark, body: Box::new(self.argument(&format!("\\{name}"))?) }
            }
            "overline" => Node::Line { over: true, body: Box::new(self.argument("\\overline")?) },
            "underline" => Node::Line { over: false, body: Box::new(self.argument("\\underline")?) },
            "limits" | "nolimits" => return err(format!("\\{name} needs a big operator right before it")),
            "" => return err("a \\ at the end"),
            other => return err(format!("the command \\{other} is not supported")),
        })
    }

    /// The delimiter after `\left` or `\right`, `None` for `.`.
    fn delimiter(&mut self, of: &str) -> Result<Option<char>, ParseError> {
        self.skip_ws();
        let Some((tok, len)) = self.look() else { return err(format!("{of} has no delimiter")) };
        self.i += len;
        match tok {
            Tok::Char('.') => Ok(None),
            Tok::Char(c @ ('(' | ')' | '[' | ']' | '|' | '/')) => Ok(Some(c)),
            Tok::Char('<') => Ok(Some('\u{27e8}')),
            Tok::Char('>') => Ok(Some('\u{27e9}')),
            Tok::Command(n) => match delimiter_symbol(&n) {
                Some(c) => Ok(Some(c)),
                None => err(format!("\\{n} is not a delimiter after {of}")),
            },
            Tok::Char(c) => err(format!("{c} is not a delimiter after {of}")),
        }
    }

    fn delimited(&mut self) -> Result<Node, ParseError> {
        let left = self.delimiter("\\left")?;
        let body = self.row(End::Right)?;
        // the row stopped before `\right`
        self.skip_ws();
        self.i += 6;
        let right = self.delimiter("\\right")?;
        Ok(Node::Delimited { left, right, body })
    }

    fn environment(&mut self) -> Result<Node, ParseError> {
        let name = self.raw_group("\\begin")?;
        let Some(env) = Env::named(&name) else { return err(format!("the environment {name} is not supported")) };
        let mut rows: Vec<Vec<Node>> = vec![Vec::new()];
        loop {
            let cell = self.row(End::Cell)?;
            if let Some(row) = rows.last_mut() {
                row.push(Node::Group(cell));
            }
            self.skip_ws();
            match self.look() {
                Some((Tok::Char('&'), _)) => self.i += 1,
                Some((Tok::Command(c), len)) if c == "\\" => {
                    self.i += len;
                    rows.push(Vec::new());
                }
                Some((Tok::Command(c), len)) if c == "end" => {
                    self.i += len;
                    let closing = self.raw_group("\\end")?;
                    if closing != name {
                        return err(format!("\\begin{{{name}}} is closed by \\end{{{closing}}}"));
                    }
                    break;
                }
                _ => return err("\\begin without \\end"),
            }
        }
        // a trailing `\\` leaves an empty last row
        if rows.len() > 1 && rows.last().is_some_and(|r| r.len() == 1 && matches!(&r[0], Node::Group(g) if g.is_empty())) {
            rows.pop();
        }
        Ok(Node::Matrix { env, rows })
    }
}

/// `c` in the math alphabet of `font`.
fn styled(c: char, font: Font) -> char {
    let at = |base: u32, off: u32| char::from_u32(base + off).unwrap_or(c);
    let greek_italic = |c: char| match c {
        '\u{3b1}'..='\u{3c9}' => at(0x1D6FC, c as u32 - 0x3B1),
        '\u{3d1}' => '\u{1D717}',
        '\u{3f0}' => '\u{1D718}',
        '\u{3d5}' => '\u{1D719}',
        '\u{3f1}' => '\u{1D71A}',
        '\u{3d6}' => '\u{1D71B}',
        '\u{3f5}' => '\u{1D716}',
        c => c,
    };
    let u = c.is_ascii_uppercase().then(|| c as u32 - 'A' as u32);
    let l = c.is_ascii_lowercase().then(|| c as u32 - 'a' as u32);
    let d = c.is_ascii_digit().then(|| c as u32 - '0' as u32);
    match font {
        Font::Roman => c,
        Font::Italic => match (u, l) {
            (Some(u), _) => at(0x1D434, u),
            (_, Some(7)) => '\u{210E}',
            (_, Some(l)) => at(0x1D44E, l),
            _ => greek_italic(c),
        },
        Font::Bold => match (u, l, d) {
            (Some(u), _, _) => at(0x1D400, u),
            (_, Some(l), _) => at(0x1D41A, l),
            (_, _, Some(d)) => at(0x1D7CE, d),
            _ => greek_italic(c),
        },
        Font::Blackboard => match (u, l, d) {
            (Some(u), _, _) => match c {
                'C' => '\u{2102}',
                'H' => '\u{210D}',
                'N' => '\u{2115}',
                'P' => '\u{2119}',
                'Q' => '\u{211A}',
                'R' => '\u{211D}',
                'Z' => '\u{2124}',
                _ => at(0x1D538, u),
            },
            (_, Some(l), _) => at(0x1D552, l),
            (_, _, Some(d)) => at(0x1D7D8, d),
            _ => c,
        },
        Font::Script => match (u, l) {
            (Some(u), _) => match c {
                'B' => '\u{212C}',
                'E' => '\u{2130}',
                'F' => '\u{2131}',
                'H' => '\u{210B}',
                'I' => '\u{2110}',
                'L' => '\u{2112}',
                'M' => '\u{2133}',
                'R' => '\u{211B}',
                _ => at(0x1D49C, u),
            },
            (_, Some(l)) => match c {
                'e' => '\u{212F}',
                'g' => '\u{210A}',
                'o' => '\u{2134}',
                _ => at(0x1D4B6, l),
            },
            _ => c,
        },
    }
}

/// Spaces in em (`\,` is 3/18 em).
fn space(name: &str) -> Option<f32> {
    Some(match name {
        "," => 3.0 / 18.0,
        ":" | ">" => 4.0 / 18.0,
        ";" => 5.0 / 18.0,
        "!" => -3.0 / 18.0,
        " " => 0.25,
        "quad" => 1.0,
        "qquad" => 2.0,
        _ => return None,
    })
}

/// The delimiters that `\left` and `\right` take as commands.
fn delimiter_symbol(name: &str) -> Option<char> {
    Some(match name {
        "{" | "lbrace" => '{',
        "}" | "rbrace" => '}',
        "langle" => '\u{27e8}',
        "rangle" => '\u{27e9}',
        "lfloor" => '\u{230a}',
        "rfloor" => '\u{230b}',
        "lceil" => '\u{2308}',
        "rceil" => '\u{2309}',
        "|" | "Vert" => '\u{2016}',
        "vert" | "lvert" | "rvert" => '|',
        "backslash" => '\\',
        "uparrow" => '\u{2191}',
        "downarrow" => '\u{2193}',
        _ => return None,
    })
}

/// A big operator: its symbol and whether it has limits above and below in display style.
fn big_operator(name: &str) -> Option<(char, bool)> {
    Some(match name {
        "sum" => ('\u{2211}', true),
        "prod" => ('\u{220f}', true),
        "coprod" => ('\u{2210}', true),
        "bigcup" => ('\u{22c3}', true),
        "bigcap" => ('\u{22c2}', true),
        "bigvee" => ('\u{22c1}', true),
        "bigwedge" => ('\u{22c0}', true),
        "bigoplus" => ('\u{2a01}', true),
        "bigotimes" => ('\u{2a02}', true),
        "bigsqcup" => ('\u{2a06}', true),
        "int" => ('\u{222b}', false),
        "iint" => ('\u{222c}', false),
        "iiint" => ('\u{222d}', false),
        "oint" => ('\u{222e}', false),
        _ => return None,
    })
}

pub(super) const FUNCTIONS: &[&str] =
    &["sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "coth", "log", "ln", "lg", "exp", "lim", "limsup", "liminf", "max", "min", "sup", "inf", "det", "gcd", "deg", "dim", "ker", "arg", "Pr"];

/// The named operators that have limits above and below in display style.
pub(super) const LIMIT_FUNCTIONS: &[&str] = &["lim", "limsup", "liminf", "max", "min", "sup", "inf", "det", "gcd", "Pr"];

/// The Greek letters and what they are called.
pub(super) const GREEK: [(&str, char); 31] = [
    ("alpha", '\u{3b1}'),
    ("beta", '\u{3b2}'),
    ("gamma", '\u{3b3}'),
    ("delta", '\u{3b4}'),
    ("varepsilon", '\u{3b5}'),
    ("zeta", '\u{3b6}'),
    ("eta", '\u{3b7}'),
    ("theta", '\u{3b8}'),
    ("iota", '\u{3b9}'),
    ("kappa", '\u{3ba}'),
    ("lambda", '\u{3bb}'),
    ("mu", '\u{3bc}'),
    ("nu", '\u{3bd}'),
    ("xi", '\u{3be}'),
    ("omicron", '\u{3bf}'),
    ("pi", '\u{3c0}'),
    ("rho", '\u{3c1}'),
    ("varsigma", '\u{3c2}'),
    ("sigma", '\u{3c3}'),
    ("tau", '\u{3c4}'),
    ("upsilon", '\u{3c5}'),
    ("varphi", '\u{3c6}'),
    ("chi", '\u{3c7}'),
    ("psi", '\u{3c8}'),
    ("omega", '\u{3c9}'),
    ("epsilon", '\u{3f5}'),
    ("vartheta", '\u{3d1}'),
    ("varkappa", '\u{3f0}'),
    ("phi", '\u{3d5}'),
    ("varrho", '\u{3f1}'),
    ("varpi", '\u{3d6}'),
];

/// The capital Greek letters that are not Latin ones.
pub(super) const CAPITALS: [(&str, char); 11] =
    [("Gamma", '\u{393}'), ("Delta", '\u{394}'), ("Theta", '\u{398}'), ("Lambda", '\u{39b}'), ("Xi", '\u{39e}'), ("Pi", '\u{3a0}'), ("Sigma", '\u{3a3}'), ("Upsilon", '\u{3a5}'), ("Phi", '\u{3a6}'), ("Psi", '\u{3a8}'), ("Omega", '\u{3a9}')];

/// The other symbols: relations, arrows, binary operators, ordinary symbols and delimiters.
pub(super) const SYMBOLS: &[(&str, char, Class)] = &[
    // relations
    ("le", '\u{2264}', Class::Rel),
    ("leq", '\u{2264}', Class::Rel),
    ("ge", '\u{2265}', Class::Rel),
    ("geq", '\u{2265}', Class::Rel),
    ("ne", '\u{2260}', Class::Rel),
    ("neq", '\u{2260}', Class::Rel),
    ("approx", '\u{2248}', Class::Rel),
    ("equiv", '\u{2261}', Class::Rel),
    ("sim", '\u{223c}', Class::Rel),
    ("simeq", '\u{2243}', Class::Rel),
    ("cong", '\u{2245}', Class::Rel),
    ("propto", '\u{221d}', Class::Rel),
    ("ll", '\u{226a}', Class::Rel),
    ("gg", '\u{226b}', Class::Rel),
    ("in", '\u{2208}', Class::Rel),
    ("notin", '\u{2209}', Class::Rel),
    ("ni", '\u{220b}', Class::Rel),
    ("subset", '\u{2282}', Class::Rel),
    ("supset", '\u{2283}', Class::Rel),
    ("subseteq", '\u{2286}', Class::Rel),
    ("supseteq", '\u{2287}', Class::Rel),
    ("perp", '\u{22a5}', Class::Rel),
    ("parallel", '\u{2225}', Class::Rel),
    ("mid", '\u{2223}', Class::Rel),
    ("vdash", '\u{22a2}', Class::Rel),
    ("models", '\u{22a8}', Class::Rel),
    ("prec", '\u{227a}', Class::Rel),
    ("succ", '\u{227b}', Class::Rel),
    ("colon", ':', Class::Punct),
    // arrows
    ("to", '\u{2192}', Class::Rel),
    ("rightarrow", '\u{2192}', Class::Rel),
    ("leftarrow", '\u{2190}', Class::Rel),
    ("gets", '\u{2190}', Class::Rel),
    ("leftrightarrow", '\u{2194}', Class::Rel),
    ("Rightarrow", '\u{21d2}', Class::Rel),
    ("Leftarrow", '\u{21d0}', Class::Rel),
    ("Leftrightarrow", '\u{21d4}', Class::Rel),
    ("implies", '\u{27f9}', Class::Rel),
    ("iff", '\u{27fa}', Class::Rel),
    ("mapsto", '\u{21a6}', Class::Rel),
    ("longrightarrow", '\u{27f6}', Class::Rel),
    ("longleftarrow", '\u{27f5}', Class::Rel),
    ("uparrow", '\u{2191}', Class::Rel),
    ("downarrow", '\u{2193}', Class::Rel),
    ("Uparrow", '\u{21d1}', Class::Rel),
    ("Downarrow", '\u{21d3}', Class::Rel),
    ("nearrow", '\u{2197}', Class::Rel),
    ("searrow", '\u{2198}', Class::Rel),
    ("hookrightarrow", '\u{21aa}', Class::Rel),
    ("rightleftharpoons", '\u{21cc}', Class::Rel),
    // binary operators
    ("pm", '\u{b1}', Class::Bin),
    ("mp", '\u{2213}', Class::Bin),
    ("times", '\u{d7}', Class::Bin),
    ("div", '\u{f7}', Class::Bin),
    ("cdot", '\u{22c5}', Class::Bin),
    ("ast", '\u{2217}', Class::Bin),
    ("star", '\u{22c6}', Class::Bin),
    ("circ", '\u{2218}', Class::Bin),
    ("bullet", '\u{2219}', Class::Bin),
    ("cup", '\u{222a}', Class::Bin),
    ("cap", '\u{2229}', Class::Bin),
    ("setminus", '\u{2216}', Class::Bin),
    ("wedge", '\u{2227}', Class::Bin),
    ("land", '\u{2227}', Class::Bin),
    ("vee", '\u{2228}', Class::Bin),
    ("lor", '\u{2228}', Class::Bin),
    ("oplus", '\u{2295}', Class::Bin),
    ("ominus", '\u{2296}', Class::Bin),
    ("otimes", '\u{2297}', Class::Bin),
    ("oslash", '\u{2298}', Class::Bin),
    ("odot", '\u{2299}', Class::Bin),
    ("sqcup", '\u{2294}', Class::Bin),
    ("sqcap", '\u{2293}', Class::Bin),
    ("uplus", '\u{228e}', Class::Bin),
    // ordinary symbols
    ("infty", '\u{221e}', Class::Ord),
    ("partial", '\u{2202}', Class::Ord),
    ("nabla", '\u{2207}', Class::Ord),
    ("emptyset", '\u{2205}', Class::Ord),
    ("varnothing", '\u{2205}', Class::Ord),
    ("forall", '\u{2200}', Class::Ord),
    ("exists", '\u{2203}', Class::Ord),
    ("nexists", '\u{2204}', Class::Ord),
    ("neg", '\u{ac}', Class::Ord),
    ("lnot", '\u{ac}', Class::Ord),
    ("angle", '\u{2220}', Class::Ord),
    ("triangle", '\u{25b3}', Class::Ord),
    ("hbar", '\u{210f}', Class::Ord),
    ("ell", '\u{2113}', Class::Ord),
    ("Re", '\u{211c}', Class::Ord),
    ("Im", '\u{2111}', Class::Ord),
    ("aleph", '\u{2135}', Class::Ord),
    ("wp", '\u{2118}', Class::Ord),
    ("prime", '\u{2032}', Class::Ord),
    ("top", '\u{22a4}', Class::Ord),
    ("bot", '\u{22a5}', Class::Ord),
    ("ldots", '\u{2026}', Class::Ord),
    ("dots", '\u{2026}', Class::Ord),
    ("cdots", '\u{22ef}', Class::Ord),
    ("vdots", '\u{22ee}', Class::Ord),
    ("ddots", '\u{22f1}', Class::Ord),
    ("therefore", '\u{2234}', Class::Rel),
    ("because", '\u{2235}', Class::Rel),
    ("square", '\u{25a1}', Class::Ord),
    ("degree", '\u{b0}', Class::Ord),
    ("backslash", '\\', Class::Ord),
    ("|", '\u{2016}', Class::Ord),
    // delimiters and escaped characters
    ("{", '{', Class::Open),
    ("}", '}', Class::Close),
    ("lbrace", '{', Class::Open),
    ("rbrace", '}', Class::Close),
    ("langle", '\u{27e8}', Class::Open),
    ("rangle", '\u{27e9}', Class::Close),
    ("lfloor", '\u{230a}', Class::Open),
    ("rfloor", '\u{230b}', Class::Close),
    ("lceil", '\u{2308}', Class::Open),
    ("rceil", '\u{2309}', Class::Close),
    ("lvert", '|', Class::Open),
    ("rvert", '|', Class::Close),
    ("vert", '|', Class::Ord),
    ("Vert", '\u{2016}', Class::Ord),
    ("%", '%', Class::Ord),
    ("$", '$', Class::Ord),
    ("&", '&', Class::Ord),
    ("#", '#', Class::Ord),
    ("_", '_', Class::Ord),
];

/// A symbol command: the character and its class.
fn symbol(name: &str) -> Option<(char, Class)> {
    if let Some((_, c)) = GREEK.iter().chain(CAPITALS.iter()).find(|(n, _)| *n == name) {
        return Some((*c, Class::Ord));
    }
    SYMBOLS.iter().find(|(n, ..)| *n == name).map(|(_, c, k)| (*c, *k))
}
